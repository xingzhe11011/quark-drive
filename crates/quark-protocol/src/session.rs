use std::collections::BTreeMap;

use cookie::Cookie;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const QUARK_COOKIE_NAMES: &[&str] = &[
    "__puus",
    "__pus",
    "__uid",
    "__kp",
    "__kps",
    "__uidc",
    "b-user-id",
    "is_star",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    cookies: BTreeMap<String, String>,
}

impl Session {
    pub fn from_cookie_header(raw: &str) -> Result<Self> {
        let mut session = Self::default();
        for part in raw.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let Some((name, value)) = part.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            session
                .cookies
                .insert(name.to_string(), value.trim().to_string());
        }

        if !session.is_authenticated() {
            return Err(Error::InvalidCookie(
                "Cookie 中缺少 __pus/__puus，请确认已登录夸克网盘".to_string(),
            ));
        }
        Ok(session)
    }

    pub fn from_cookie_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Result<Self> {
        let mut session = Self::default();
        for (name, value) in pairs {
            if name.is_empty() {
                continue;
            }
            session.cookies.insert(name, value);
        }
        if !session.is_authenticated() {
            return Err(Error::InvalidCookie(
                "登录窗口未返回有效 Cookie，请重新扫码登录".to_string(),
            ));
        }
        Ok(session)
    }

    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn is_authenticated(&self) -> bool {
        ["__puus", "__pus"].iter().any(|name| {
            self.cookies
                .get(*name)
                .map(|value| !value.is_empty())
                .unwrap_or(false)
        })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(String::as_str)
    }

    pub fn apply_headers(&mut self, headers: &HeaderMap) {
        for value in headers.get_all(reqwest::header::SET_COOKIE).iter() {
            if let Ok(text) = value.to_str() {
                self.apply_set_cookie(text);
            }
        }
    }

    pub fn apply_set_cookie(&mut self, raw: &str) {
        let Ok(parsed) = Cookie::parse(raw.to_string()) else {
            return;
        };
        let name = parsed.name().to_string();
        if !QUARK_COOKIE_NAMES.contains(&name.as_str()) && !name.starts_with("__") {
            return;
        }
        if parsed.value().is_empty() {
            self.cookies.remove(&name);
        } else {
            self.cookies.insert(name, parsed.value().to_string());
        }
    }

    pub fn redacted(&self) -> String {
        self.cookies
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "__puus" | "__pus" | "__kp"))
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cookie_header() {
        let session = Session::from_cookie_header("__puus=abc; __pus=def; foo=bar").unwrap();
        assert!(session.is_authenticated());
        assert_eq!(session.get("__puus"), Some("abc"));
        assert_eq!(session.get("foo"), Some("bar"));
    }

    #[test]
    fn rejects_cookie_without_auth_cookie() {
        assert!(Session::from_cookie_header("foo=bar").is_err());
    }

    #[test]
    fn accepts_login_cookie_before_puus_rotation() {
        let session = Session::from_cookie_header("__pus=pus; __kps=kps; __uid=42").unwrap();
        assert!(session.is_authenticated());
        assert_eq!(session.get("__puus"), None);
    }

    #[test]
    fn rotates_puus_from_set_cookie() {
        let mut session = Session::from_cookie_header("__puus=old").unwrap();
        session.apply_set_cookie("__puus=new; Path=/; HttpOnly; Secure");
        assert_eq!(session.get("__puus"), Some("new"));
    }
}
