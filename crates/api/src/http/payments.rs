//! Tournament-only Stripe test checkout. Never accepts a live credential or client prices.
use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde_json::{json, Value};
use crate::AppState;

type PaymentError = (StatusCode, Json<Value>);
fn error(status: StatusCode, message: &str) -> PaymentError {
    (status, Json(json!({"message":message})))
}
fn test_key(key: &str) -> bool {
    key.starts_with("sk_test_") || key.starts_with("rk_test_")
}
fn key() -> Result<String, PaymentError> {
    let key = std::env::var("STRIPE_TEST_SECRET_KEY").unwrap_or_default();
    if !test_key(&key) { return Err(error(StatusCode::SERVICE_UNAVAILABLE, "Test checkout is not configured")); }
    Ok(key)
}
fn client() -> Result<reqwest::Client, PaymentError> {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none()).build()
        .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE,"Checkout unavailable"))
}
async fn response(response: Result<reqwest::Response, reqwest::Error>) -> Result<Value, PaymentError> {
    let response = response.map_err(|_| error(StatusCode::BAD_GATEWAY,"Stripe unavailable"))?;
    if !response.status().is_success() { return Err(error(StatusCode::BAD_GATEWAY,"Stripe request failed")); }
    response.json().await.map_err(|_| error(StatusCode::BAD_GATEWAY,"Invalid Stripe response"))
}
pub async fn create(State(state): State<AppState>) -> Result<Json<Value>, PaymentError> {
    let key = key()?;
    let base = &state.config.public_web_url;
    if !base.starts_with("https://") { return Err(error(StatusCode::SERVICE_UNAVAILABLE,"Public HTTPS URL required")); }
    let success = format!("{base}/checkout?session_id={{CHECKOUT_SESSION_ID}}");
    let cancel = format!("{base}/checkout?cancelled=1");
    let result = response(client()?.post("https://api.stripe.com/v1/checkout/sessions")
        .bearer_auth(key).form(&[
            ("mode","payment"), ("payment_method_types[0]","card"),
            ("line_items[0][price_data][currency]","jpy"),
            ("line_items[0][price_data][unit_amount]","100"),
            ("line_items[0][price_data][product_data][name]","Nothing to See Here"),
            ("line_items[0][quantity]","1"), ("success_url",success.as_str()),
            ("cancel_url",cancel.as_str()), ("metadata[application]","nullbox-test"),
        ]).send().await).await?;
    let url = result["url"].as_str().unwrap_or_default();
    if result["livemode"] != false || !url.starts_with("https://checkout.stripe.com/") {
        return Err(error(StatusCode::BAD_GATEWAY,"Invalid test checkout"));
    }
    Ok(Json(json!({"url":url})))
}
pub async fn status(Path(id): Path<String>) -> Result<Json<Value>, PaymentError> {
    if !id.starts_with("cs_test_") || id.len()>200 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c==b'_') {
        return Err(error(StatusCode::BAD_REQUEST,"Invalid test session"));
    }
    let result = response(client()?.get(format!("https://api.stripe.com/v1/checkout/sessions/{id}"))
        .bearer_auth(key()?).send().await).await?;
    if result["livemode"] != false || result["metadata"]["application"] != "nullbox-test" {
        return Err(error(StatusCode::BAD_REQUEST,"Not a Nullbox test session"));
    }
    Ok(Json(json!({"paid":result["payment_status"]=="paid" && result["status"]=="complete"})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn only_test_credentials() {
        assert!(test_key("rk_test_example")); assert!(test_key("sk_test_example"));
        assert!(!test_key("sk_live_example")); assert!(!test_key("rk_live_example")); assert!(!test_key(""));
    }
}
