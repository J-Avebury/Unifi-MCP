mod compatibility;

use crate::{BASE_TOOLS, ToolSpec};
use compatibility::COMPATIBILITY_TOOLS;

pub(crate) fn iter() -> impl Iterator<Item = &'static ToolSpec> {
    BASE_TOOLS.iter().chain(COMPATIBILITY_TOOLS.iter())
}

pub(crate) fn find(name: &str) -> Option<&'static ToolSpec> {
    iter().find(|tool| tool.name == name)
}

#[cfg(test)]
pub(crate) fn is_compatibility(name: &str) -> bool {
    COMPATIBILITY_TOOLS.iter().any(|tool| tool.name == name)
}
