//! OAuth authentication and token persistence.

use async_trait::async_trait;

use super::Extension;

pub struct AuthExtension;

#[async_trait]
impl Extension for AuthExtension {
    fn name(&self) -> &str { "auth" }
    fn description(&self) -> &str { "OAuth authentication and token persistence" }
}
