mod compatibility;

use crate::{BASE_TOOLS, ToolSpec};
use compatibility::COMPATIBILITY_TOOLS;

pub(crate) fn iter() -> impl Iterator<Item = &'static ToolSpec> {
    BASE_TOOLS.iter().chain(COMPATIBILITY_TOOLS.iter())
}

pub(crate) fn find(name: &str) -> Option<&'static ToolSpec> {
    iter().find(|tool| tool.name == name)
}
