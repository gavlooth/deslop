use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUser {
    pub id: String,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUser {
    pub id: String,
    pub name: String,
    pub email: String,
}

pub trait Handler<T> {
    fn handle(&self, input: T) -> Result<(), String>;
}

pub trait Validator<T> {
    fn validate(&self, input: T) -> Result<(), String>;
}

impl Handler<CreateUser> for () {
    fn handle(&self, _input: CreateUser) -> Result<(), String> {
        Ok(())
    }
}

impl Validator<UpdateUser> for () {
    fn validate(&self, _input: UpdateUser) -> Result<(), String> {
        Ok(())
    }
}
