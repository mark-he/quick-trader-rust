use crate::http::{Credentials, Method};

#[derive(PartialEq, Eq, Debug)]
pub struct Request {
    pub(crate) method: Method,
    pub(crate) path: String,
    pub(crate) params: Vec<(String, String)>,
    pub(crate) credentials: Option<Credentials>,
    pub(crate) sign: bool,
    pub(crate) body: String,
    pub(crate) recv_window: u32,
    
}

impl Request {
    pub fn method(&self) -> &Method {
        &self.method
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn body(&self) -> &str {
        &self.body
    }
    pub fn params(&self) -> &[(String, String)] {
        &self.params
    }
    pub fn credentials(&self) -> &Option<Credentials> {
        &self.credentials
    }
    pub fn sign(&self) -> &bool {
        &self.sign
    }
    pub fn recv_window(&self) -> &u32 {
        &self.recv_window
    }
}

/// /// API HTTP Request
///
/// A low-level request builder for API integration
/// decoupled from any specific underlying HTTP library.
pub struct RequestBuilder {
    method: Method,
    path: String,
    params: Vec<(String, String)>,
    credentials: Option<Credentials>,
    sign: bool,
    body: String,
    recv_window: u32,
}

impl RequestBuilder {
    pub fn new(method: Method, path: &str) -> Self {
        Self {
            method,
            path: path.to_owned(),
            params: vec![],
            body: "".to_string(),
            credentials: None,
            sign: false,
            recv_window: 5000,
        }
    }

    /// Append `params` to the request's query string. Parameters may
    /// share the same key, and will result in a query string with one or
    /// more duplicated query parameter keys.
    pub fn params<'a>(mut self, params: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        self.params.extend(
            params
                .into_iter()
                .map(|param| (param.0.to_owned(), param.1.to_owned())),
        );

        self
    }

    pub fn credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);

        self
    }

    pub fn sign(mut self) -> Self {
        self.sign = true;

        self
    }

    pub fn recv_window(mut self, recv_window: u32) -> Self {
        self.recv_window = recv_window;

        self
    }

    pub fn body(mut self, body: &str) -> Self {
        self.body = body.to_string();

        self
    }
}

impl From<RequestBuilder> for Request {
    fn from(builder: RequestBuilder) -> Request {
        Request {
            method: builder.method,
            path: builder.path,
            params: builder.params,
            credentials: builder.credentials,
            sign: builder.sign,
            body: builder.body,
            recv_window: builder.recv_window,
        }
    }
}
