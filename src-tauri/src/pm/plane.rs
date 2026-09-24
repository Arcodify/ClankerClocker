use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneUser {
    pub id: String,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub last_name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneProject {
    pub id: String,
    pub name: String,
    pub identifier: String,
    /// Whether the API key's own user is a member of this project. The
    /// workspace projects endpoint returns every project in the workspace,
    /// not just the caller's — there is no server-side "mine only" filter
    /// (confirmed against the live instance), so callers must filter on
    /// this themselves. See `Plane::list_my_projects`.
    #[serde(default)]
    pub is_member: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneIssue {
    pub id: String,
    pub name: String,
    pub project: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub sequence_id: Option<i64>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    /// Non-null when this issue is a sub-issue of another. Used to render
    /// a project's issue list as a tree instead of a flat list where
    /// sub-issues would otherwise appear as unrelated top-level rows.
    #[serde(default)]
    pub parent: Option<String>,
}

/// A workspace member, resolved separately from issues since Plane's issue
/// payloads only carry member ids (`assignees`, `created_by`), never names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneMember {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneState {
    pub id: String,
    pub name: String,
    /// One of "backlog" | "unstarted" | "started" | "completed" |
    /// "cancelled" (confirmed against the live instance). "completed" is
    /// the group `complete_issue` looks for.
    pub group: String,
}

/// Plane's list endpoints are cursor-paginated (`next_cursor`/
/// `next_page_results`), not page-numbered like PocketBase's.
#[derive(Debug, Deserialize)]
struct PlanePage<T> {
    results: Vec<T>,
    #[serde(default)]
    next_cursor: Option<String>,
    #[serde(default)]
    next_page_results: bool,
}

#[derive(Debug, Clone)]
pub struct Plane {
    base_url: String,
    workspace_slug: String,
    client: Client,
    user_token: String,
}

impl Plane {
    pub fn new(base_url: String, workspace_slug: String, user_token: String) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            workspace_slug,
            user_token,
        }
    }

    fn ws_url(&self, path: &str) -> String {
        format!(
            "{}/api/v1/workspaces/{}/{}",
            self.base_url, self.workspace_slug, path
        )
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T> {
        let resp = self
            .client
            .get(url)
            .header("X-API-Key", &self.user_token)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Plane GET {url} failed ({status}): {text}"));
        }
        Ok(resp.json::<T>().await?)
    }

    async fn post_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let resp = self
            .client
            .post(url)
            .header("X-API-Key", &self.user_token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Plane POST {url} failed ({status}): {text}"));
        }
        Ok(resp.json::<T>().await?)
    }

    async fn patch_json(&self, url: &str, body: serde_json::Value) -> Result<()> {
        let resp = self
            .client
            .patch(url)
            .header("X-API-Key", &self.user_token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Plane PATCH {url} failed ({status}): {text}"));
        }
        Ok(())
    }

    /// Follows Plane's cursor pagination until `next_page_results` is false.
    async fn get_all_pages<T: for<'de> Deserialize<'de>>(&self, base_url: &str) -> Result<Vec<T>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let url = match &cursor {
                Some(c) => {
                    let sep = if base_url.contains('?') { "&" } else { "?" };
                    format!("{base_url}{sep}cursor={c}")
                }
                None => base_url.to_string(),
            };
            let page: PlanePage<T> = self.get_json(&url).await?;
            let more = page.next_page_results;
            let next = page.next_cursor.clone();
            all.extend(page.results);
            if !more || next.is_none() {
                break;
            }
            cursor = next;
        }
        Ok(all)
    }

    pub async fn current_user(&self) -> Result<PlaneUser> {
        self.get_json(&format!("{}/api/v1/users/me/", self.base_url))
            .await
    }

    /// Every member of the workspace, for resolving assignee/creator ids on
    /// issues to display names. This endpoint returns a plain JSON array
    /// (confirmed against the live instance), not the cursor-paginated
    /// envelope every other list endpoint uses.
    pub async fn list_workspace_members(&self) -> Result<Vec<PlaneMember>> {
        self.get_json(&self.ws_url("members/")).await
    }

    /// Projects the API key's own user is actually a member of.
    pub async fn list_my_projects(&self) -> Result<Vec<PlaneProject>> {
        let all: Vec<PlaneProject> = self.get_all_pages(&self.ws_url("projects/")).await?;
        Ok(all.into_iter().filter(|p| p.is_member).collect())
    }

    pub async fn list_project_issues(&self, project_id: &str) -> Result<Vec<PlaneIssue>> {
        self.get_all_pages(&self.ws_url(&format!("projects/{project_id}/issues/")))
            .await
    }

    /// Issues assigned to `my_user_id` across every project the caller is a
    /// member of, newest-updated first. Plane's issues-list endpoint does
    /// not filter server-side by assignee (confirmed against the live
    /// instance — passing `?assignees=` had no effect), so this fans out
    /// per project and filters/sorts client-side, capped to `limit`.
    pub async fn list_recent_assigned_issues(
        &self,
        my_user_id: &str,
        limit: usize,
    ) -> Result<Vec<PlaneIssue>> {
        let projects = self.list_my_projects().await?;
        let mut tasks = tokio::task::JoinSet::new();
        for project in projects {
            let plane = self.clone();
            tasks.spawn(async move { plane.list_project_issues(&project.id).await });
        }
        let mut issues = Vec::new();
        while let Some(res) = tasks.join_next().await {
            if let Ok(Ok(project_issues)) = res {
                issues.extend(project_issues);
            }
        }
        let my_user_id = my_user_id.to_string();
        issues.retain(|i| i.assignees.iter().any(|a| *a == my_user_id));
        issues.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        issues.truncate(limit);
        Ok(issues)
    }

    pub async fn create_issue(&self, project_id: &str, name: &str) -> Result<PlaneIssue> {
        self.post_json(
            &self.ws_url(&format!("projects/{project_id}/issues/")),
            json!({ "name": name }),
        )
        .await
    }

    pub async fn project_states(&self, project_id: &str) -> Result<Vec<PlaneState>> {
        self.get_all_pages(&self.ws_url(&format!("projects/{project_id}/states/")))
            .await
    }

    /// Marks an issue done by moving it to the project's "completed"-group
    /// state. Plane has no boolean "done" flag on an issue — completion is
    /// just being in whichever per-project state has `group == "completed"`.
    pub async fn complete_issue(&self, project_id: &str, issue_id: &str) -> Result<()> {
        let states = self.project_states(project_id).await?;
        let done = states
            .iter()
            .find(|s| s.group == "completed")
            .ok_or_else(|| anyhow!("project {project_id} has no 'completed' state"))?;
        self.patch_json(
            &self.ws_url(&format!("projects/{project_id}/issues/{issue_id}/")),
            json!({ "state": done.id }),
        )
        .await
    }
}
