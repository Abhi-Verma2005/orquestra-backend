use uuid::Uuid;

use crate::models::Role;

pub fn generate_id(role: Role) -> String {
    let uuid = Uuid::new_v4().to_string();
    let uuid_8char = uuid
        .split("-")
        .next()
        .unwrap();
    match role {
        Role::Assistant => {
            format!("assisstant_{}", uuid_8char)
        }
        Role::System => {
            format!("system_{}", uuid_8char)
        }
        Role::User => {
            format!("user_{}", uuid_8char)
        }
        _ => {
            format!("msg_{}", uuid_8char)
        }
    }
}