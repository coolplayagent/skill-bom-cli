//! The password destination is fixed and cannot be overridden by configuration.
use super::{AuthMode, Http, Response};
use crate::domain::Result;
use reqwest::{Method, blocking::RequestBuilder};

pub const W3_LOGIN_URL: &str =
    "https://rnd-idea-api.huawei.com/ideaclientservice/login/v4/secureLogin";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub(super) fn headers(request: RequestBuilder) -> RequestBuilder {
    request
        .header("Accept", "application/json, text/javascript, */*; q=0.01")
        .header("Accept-Language", "zh-CN,zh")
        .header("User-Agent", USER_AGENT)
}
impl Http {
    pub fn secure_login(&self, username: &str, password: &str) -> Result<Response> {
        self.login_at(W3_LOGIN_URL, username, password)
    }
    pub(super) fn login_at(&self, url: &str, username: &str, password: &str) -> Result<Response> {
        let mut body =
            serde_json::json!({"user":username,"password":password,"requireUserInfo":"true"});
        let result = self.request(Method::POST, url, Some(&body), None, AuthMode::Login);
        // reqwest owns the encoded body during I/O. Wipe our retained JSON password.
        if let Some(serde_json::Value::String(secret)) = body.get_mut("password") {
            zeroize::Zeroize::zeroize(secret);
        }
        result
    }
}

#[cfg(test)]
#[path = "login_tests.rs"]
mod tests;
