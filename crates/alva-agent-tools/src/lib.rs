// INPUT:  alva_types::ToolRegistry, all tool modules
// OUTPUT: register_builtin_tools, register_all_tools
// POS:    Crate root — declares tool modules and provides registration functions.
//! Built-in tool implementations for the agent framework.
//!
//! Standard tools (always available):
//!   ask_human, create_file, execute_shell, file_edit, find_files,
//!   grep_search, list_files, read_file, view_image
//!
//! Native-only tools (feature = "native", disabled on wasm):
//!   internet_search, read_url
//!
//! Browser tools (feature = "browser"):
//!   browser_start, browser_stop, browser_navigate, browser_action,
//!   browser_snapshot, browser_screenshot, browser_status

pub mod ask_human;
pub mod create_file;
pub mod execute_shell;
pub mod file_edit;
pub mod find_files;
pub mod grep_search;
pub mod list_files;
pub mod mock_fs;
pub mod read_file;
pub mod truncate;
pub mod view_image;

#[cfg(not(target_family = "wasm"))]
pub mod local_fs;
#[cfg(not(target_family = "wasm"))]
pub use local_fs::{walk_dir, LocalToolFs};

pub use mock_fs::MockToolFs;

#[cfg(feature = "native")]
pub mod internet_search;
#[cfg(feature = "native")]
pub mod read_url;

#[cfg(feature = "browser")]
pub mod browser;

use alva_types::ToolRegistry;

/// Batch-register tools into a ToolRegistry.
#[macro_export]
macro_rules! register_tools {
    ($registry:expr, $($tool:expr),* $(,)?) => {
        $( $registry.register(Box::new($tool)); )*
    };
}

/// Register all built-in tools into a ToolRegistry
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    register_tools!(
        registry,
        execute_shell::ExecuteShellTool,
        create_file::CreateFileTool,
        file_edit::FileEditTool,
        read_file::ReadFileTool,
        find_files::FindFilesTool,
        grep_search::GrepSearchTool,
        list_files::ListFilesTool,
        ask_human::AskHumanTool,
        view_image::ViewImageTool,
    );

    #[cfg(feature = "native")]
    register_tools!(
        registry,
        internet_search::InternetSearchTool,
        read_url::ReadUrlTool,
    );
}

/// Register all built-in tools including browser tools into a ToolRegistry.
///
/// Browser tools share a `BrowserManager` instance for coordinating Chrome lifecycle.
#[cfg(feature = "browser")]
pub fn register_all_tools(registry: &mut ToolRegistry) {
    // Standard tools
    register_builtin_tools(registry);

    // Browser tools — all share the same BrowserManager
    let manager = browser::browser_manager::shared_browser_manager();
    registry.register(Box::new(browser::BrowserStartTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserStopTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserNavigateTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserActionTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserSnapshotTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserScreenshotTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserStatusTool {
        manager: manager.clone(),
    }));
}

#[cfg(not(feature = "browser"))]
pub fn register_all_tools(registry: &mut ToolRegistry) {
    register_builtin_tools(registry);
}
