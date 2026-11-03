use crate::env::env_vars::{SUPABASE_SECRET_KEY, SUPABASE_URL};
use reqwest::{
    Method, StatusCode,
    header::{AUTHORIZATION, CONTENT_TYPE},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{fmt::Display, sync::LazyLock, time::Duration};

// Our servers reach the database only through PostgREST, with the service key. This is the only code that knows its URL, its
// headers, its filter syntax and how it reports errors.

#[derive(Debug)]
pub struct PostgrestError {
    request: String,
    // None when PostgREST never answered, or its answer couldn't be read.
    status: Option<StatusCode>,
    // The SQLSTATE the database raised. Our functions raise 'PT<status>' for an error meant for the user.
    code: Option<String>,
    pub message: String,
}

impl PostgrestError {
    // The status to show the user along with the message, when a database function raised this error for them.
    pub fn user_status(&self) -> Option<StatusCode> {
        self.status.filter(|_| self.code.as_deref().is_some_and(|code| code.starts_with("PT")))
    }
}

impl Display for PostgrestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(status) => write!(formatter, "{} failed (status {status}): {}", self.request, self.message),
            None => write!(formatter, "{} failed: {}", self.request, self.message),
        }
    }
}

impl std::error::Error for PostgrestError {}

// Calls a database function and reads what it returns. A function that returns nothing reads as ().
pub async fn call<Output: DeserializeOwned>(function: &str, arguments: impl Serialize) -> Result<Output, PostgrestError> {
    let path = format!("rpc/{function}");
    let bytes = send(Method::POST, &path, &[], None, Some(&arguments)).await?;
    decode(&format!("POST {path}"), &bytes)
}

pub fn from(table: &str) -> Query {
    Query {
        table: table.to_string(),
        parameters: Vec::new(),
    }
}

pub struct Query {
    table: String,
    parameters: Vec<(String, String)>,
}

impl Query {
    pub fn select(self, columns: &str) -> Self {
        self.with("select", columns.to_string())
    }

    pub fn eq(self, column: &str, value: impl Display) -> Self {
        self.with(column, format!("eq.{value}"))
    }

    pub fn is_in<Value: Display>(self, column: &str, values: impl IntoIterator<Item = Value>) -> Self {
        let values = values.into_iter().map(|value| format!("\"{value}\"")).collect::<Vec<_>>().join(",");
        self.with(column, format!("in.({values})"))
    }

    pub fn order_desc(self, column: &str) -> Self {
        self.with("order", format!("{column}.desc"))
    }

    pub fn limit(self, count: usize) -> Self {
        self.with("limit", count.to_string())
    }

    pub async fn get<Row: DeserializeOwned>(self) -> Result<Vec<Row>, PostgrestError> {
        let bytes = send(Method::GET, &self.table, &self.parameters, None, None::<&()>).await?;
        decode(&format!("GET {}", self.table), &bytes)
    }

    pub async fn insert(self, rows: impl Serialize) -> Result<(), PostgrestError> {
        send(Method::POST, &self.table, &self.parameters, Some("return=minimal"), Some(&rows)).await?;
        Ok(())
    }

    pub async fn insert_returning<Row: DeserializeOwned>(self, rows: impl Serialize) -> Result<Vec<Row>, PostgrestError> {
        let bytes = send(Method::POST, &self.table, &self.parameters, Some("return=representation"), Some(&rows)).await?;
        decode(&format!("POST {}", self.table), &bytes)
    }

    pub async fn update(self, changes: impl Serialize) -> Result<(), PostgrestError> {
        send(Method::PATCH, &self.table, &self.parameters, Some("return=minimal"), Some(&changes)).await?;
        Ok(())
    }

    pub async fn update_returning<Row: DeserializeOwned>(self, changes: impl Serialize) -> Result<Vec<Row>, PostgrestError> {
        let bytes = send(Method::PATCH, &self.table, &self.parameters, Some("return=representation"), Some(&changes)).await?;
        decode(&format!("PATCH {}", self.table), &bytes)
    }

    pub async fn delete(self) -> Result<(), PostgrestError> {
        send(Method::DELETE, &self.table, &self.parameters, Some("return=minimal"), None::<&()>).await?;
        Ok(())
    }

    fn with(mut self, name: &str, value: String) -> Self {
        self.parameters.push((name.to_string(), value));
        self
    }
}

// TODO: AI-PICKED-VALUE: A request PostgREST hasn't answered in 30 seconds fails.
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("PostgREST HTTP client")
});

async fn send(
    method: Method,
    path: &str,
    parameters: &[(String, String)],
    prefer: Option<&str>,
    body: Option<&impl Serialize>,
) -> Result<Vec<u8>, PostgrestError> {
    let request = format!("{method} {path}");
    let failed = |status: Option<StatusCode>, code: Option<String>, message: String| PostgrestError {
        request: request.clone(),
        status,
        code,
        message,
    };
    let mut url =
        reqwest::Url::parse(&format!("{}/rest/v1/{path}", SUPABASE_URL.trim_end_matches('/'))).map_err(|error| failed(None, None, error.to_string()))?;
    url.query_pairs_mut().extend_pairs(parameters);
    let mut builder = HTTP_CLIENT
        .request(method, url)
        .header("apikey", SUPABASE_SECRET_KEY.as_str())
        .header(AUTHORIZATION, format!("Bearer {}", SUPABASE_SECRET_KEY.as_str()))
        .header(CONTENT_TYPE, "application/json");
    if let Some(prefer) = prefer {
        builder = builder.header("Prefer", prefer);
    }
    if let Some(body) = body {
        builder = builder.body(serde_json::to_vec(body).map_err(|error| failed(None, None, error.to_string()))?);
    }
    let response = builder.send().await.map_err(|error| failed(None, None, error.to_string()))?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(|error| failed(Some(status), None, error.to_string()))?;
    if status.is_success() {
        return Ok(bytes.to_vec());
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        code: Option<String>,
        message: Option<String>,
    }
    let error_body = serde_json::from_slice::<ErrorBody>(&bytes).ok();
    let message = error_body
        .as_ref()
        .and_then(|error_body| error_body.message.clone())
        .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
    Err(failed(Some(status), error_body.and_then(|error_body| error_body.code), message))
}

fn decode<Output: DeserializeOwned>(request: &str, bytes: &[u8]) -> Result<Output, PostgrestError> {
    let bytes = if bytes.is_empty() { b"null".as_slice() } else { bytes };
    serde_json::from_slice(bytes).map_err(|error| PostgrestError {
        request: request.to_string(),
        status: None,
        code: None,
        message: format!("Unexpected answer: {error}"),
    })
}
