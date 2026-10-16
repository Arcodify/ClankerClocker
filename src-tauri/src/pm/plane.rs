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
    /// "urgent" | "high" | "medium" | "low" | "none".
    #[serde(default)]
    pub priority: Option<String>,
    /// Label ids — resolve names via `list_labels`.
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub target_date: Option<String>,
    #[serde(default)]
    pub description_stripped: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneModule {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub target_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneCycle {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneLabel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// Fields for a new issue. Everything but `name` is optional; `None`/empty
/// values are left out of the request so Plane applies its own defaults.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewIssue {
    pub name: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub target_date: Option<String>,
    #[serde(default)]
    pub module_id: Option<String>,
    #[serde(default)]
    pub cycle_id: Option<String>,
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

    async fn post_unit(&self, url: &str, body: serde_json::Value) -> Result<()> {
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
        Ok(())
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
        self.create_issue_full(
            project_id,
            NewIssue {
                name: name.to_string(),
                ..Default::default()
            },
        )
        .await
    }

    /// Creates an issue, then attaches it to a module and/or cycle if asked —
    /// Plane has no field on the issue itself for either, membership lives
    /// on the module-issues / cycle-issues endpoints.
    pub async fn create_issue_full(&self, project_id: &str, new: NewIssue) -> Result<PlaneIssue> {
        let mut body = json!({ "name": new.name });
        let non_empty = |v: &Option<String>| v.as_deref().filter(|s| !s.is_empty()).map(String::from);
        if let Some(v) = non_empty(&new.priority) {
            body["priority"] = json!(v);
        }
        if let Some(v) = non_empty(&new.state) {
            body["state"] = json!(v);
        }
        if let Some(v) = non_empty(&new.parent) {
            body["parent"] = json!(v);
        }
        if let Some(v) = non_empty(&new.target_date) {
            body["target_date"] = json!(v);
        }
        if !new.assignees.is_empty() {
            body["assignees"] = json!(new.assignees);
        }
        if !new.labels.is_empty() {
            body["labels"] = json!(new.labels);
        }
        let issue: PlaneIssue = self
            .post_json(&self.ws_url(&format!("projects/{project_id}/issues/")), body)
            .await?;
        if let Some(module_id) = non_empty(&new.module_id) {
            self.post_unit(
                &self.ws_url(&format!("projects/{project_id}/modules/{module_id}/module-issues/")),
                json!({ "issues": [issue.id] }),
            )
            .await?;
        }
        if let Some(cycle_id) = non_empty(&new.cycle_id) {
            self.post_unit(
                &self.ws_url(&format!("projects/{project_id}/cycles/{cycle_id}/cycle-issues/")),
                json!({ "issues": [issue.id] }),
            )
            .await?;
        }
        Ok(issue)
    }

    /// Partial update — `patch` is passed straight through, so callers send
    /// only the fields they're changing (`state`, `priority`, `assignees`…).
    pub async fn update_issue(
        &self,
        project_id: &str,
        issue_id: &str,
        patch: serde_json::Value,
    ) -> Result<()> {
        self.patch_json(
            &self.ws_url(&format!("projects/{project_id}/issues/{issue_id}/")),
            patch,
        )
        .await
    }

    pub async fn project_states(&self, project_id: &str) -> Result<Vec<PlaneState>> {
        self.get_all_pages(&self.ws_url(&format!("projects/{project_id}/states/")))
            .await
    }

    pub async fn list_labels(&self, project_id: &str) -> Result<Vec<PlaneLabel>> {
        self.get_all_pages(&self.ws_url(&format!("projects/{project_id}/labels/")))
            .await
    }

    pub async fn list_modules(&self, project_id: &str) -> Result<Vec<PlaneModule>> {
        self.get_all_pages(&self.ws_url(&format!("projects/{project_id}/modules/")))
            .await
    }

    pub async fn list_module_issues(&self, project_id: &str, module_id: &str) -> Result<Vec<PlaneIssue>> {
        self.get_all_pages(&self.ws_url(&format!(
            "projects/{project_id}/modules/{module_id}/module-issues/"
        )))
        .await
    }

    pub async fn list_cycles(&self, project_id: &str) -> Result<Vec<PlaneCycle>> {
        self.get_all_pages(&self.ws_url(&format!("projects/{project_id}/cycles/")))
            .await
    }

    pub async fn list_cycle_issues(&self, project_id: &str, cycle_id: &str) -> Result<Vec<PlaneIssue>> {
        self.get_all_pages(&self.ws_url(&format!(
            "projects/{project_id}/cycles/{cycle_id}/cycle-issues/"
        )))
        .await
    }

    /// Every open-or-closed issue assigned to `my_user_id` across the
    /// caller's projects. Same client-side fan-out as
    /// `list_recent_assigned_issues`, without the cap.
    pub async fn list_my_issues(&self, my_user_id: &str) -> Result<Vec<PlaneIssue>> {
        self.list_recent_assigned_issues(my_user_id, usize::MAX).await
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
        self.update_issue(project_id, issue_id, json!({ "state": done.id }))
            .await
    }
}
