use reqwest::Client;
use serde::{Deserialize, Serialize};

// struct User {
//     email: String,
//     name: String,
// }

#[derive(Serialize, Deserialize, Debug)]
pub struct PlaneUserRespnse {
    id: String,
    first_name: String,
    last_name: String,
    email: String,
    avatar: Option<String>,
    avatar_url: Option<String>,
    display_name: Option<String>,
}

// #[derive(Debug, Clone)]
// struct PlaneProject {}

#[derive(Debug, Clone)]
pub struct Plane {
    base_url: String,
    client: Client,
    // _workspace_slug: String,
    user_token: String,
}

impl Plane {
    pub fn new(_base_url: String, _workspace_slug: String, _user_token: String) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://projects.arcodify.com".to_string(),
            // _workspace_slug: "Arcodify".to_string(),
            user_token: "plane_api_d41799b496884a1197dc58c8bc0ab461".to_string(),
        }
    }

    pub async fn is_plane_user(&self) -> PlaneUserRespnse {
        let response = self
            .client
            .get(format!("{}/api/v1/users/me/", self.base_url))
            .header("X-API-Key".to_string(), &self.user_token)
            .send()
            .await
            .expect("Failed to send the reqwest");

        let status = response.status();

        let data = response
            .json::<PlaneUserRespnse>()
            .await
            .expect("Failed to parse the response as JSON");

        println!("Response: {:?}, status:: {}", data, status);
        // log::info!("Response: {:?}", respon        let status = response.status();
        data
    }

    // fn get(&self) {}

    // fn post(&self) {}
}
