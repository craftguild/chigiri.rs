//! A domain crate with ordinary Rust values and errors, independent of Chigiri.

#[derive(Clone)]
pub struct User {
    pub name: String,
    pub surname: String,
    pub given_name: String,
}

#[derive(Debug)]
pub enum AppError {
    NotFound,
    Backend(std::io::Error),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "private domain diagnostic: {self:?}")
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotFound => None,
            Self::Backend(source) => Some(source),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(source: std::io::Error) -> Self {
        Self::Backend(source)
    }
}

pub struct Users;

impl Users {
    pub async fn find(&self, id: &str) -> Result<User, AppError> {
        match id {
            "missing" => Err(AppError::NotFound),
            "internal" => Err(std::io::Error::other("private backend credential").into()),
            _ => Ok(User {
                name: "Ada".to_owned(),
                surname: "Lovelace".to_owned(),
                given_name: "Ada".to_owned(),
            }),
        }
    }
}
