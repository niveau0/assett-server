use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{AppState, metadata::hash_key};

#[derive(Clone, Debug)]
pub enum Auth {
    Anonymous,
    Admin,
    Project(String),
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let key = req
        .headers()
        .get("X-Api-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let auth = match key {
        None => Auth::Anonymous,
        Some(key) if key == state.config.api_key => Auth::Admin,
        Some(key) => {
            let hash = hash_key(&key);
            let metadata = state.metadata.read().await;
            match metadata
                .projects
                .iter()
                .find(|(_, project)| project.api_key_sha256 == hash)
            {
                Some((name, _)) => Auth::Project(name.clone()),
                None => {
                    return (StatusCode::UNAUTHORIZED, "Unauthorized\n").into_response();
                }
            }
        }
    };

    req.extensions_mut().insert(auth);
    next.run(req).await
}
