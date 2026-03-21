// INPUT:  crate::ports::tool, browser::browser_manager
// OUTPUT: register_builtin_tools, register_all_tools
// POS:    Registers all built-in tools (9 standard + 7 browser) into a ToolRegistry.
pub mod ask_human;
pub mod browser;
pub mod create_file;
pub mod execute_shell;
pub mod file_edit;
pub mod grep_search;
pub mod internet_search;
pub mod list_files;
pub mod read_url;
pub mod view_image;

use crate::ports::tool::ToolRegistry;

/// Register all built-in tools into a ToolRegistry
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    registry.register(Box::new(execute_shell::ExecuteShellTool));
    registry.register(Box::new(create_file::CreateFileTool));
    registry.register(Box::new(file_edit::FileEditTool));
    registry.register(Box::new(grep_search::GrepSearchTool));
    registry.register(Box::new(list_files::ListFilesTool));
    registry.register(Box::new(ask_human::AskHumanTool));
    registry.register(Box::new(internet_search::InternetSearchTool));
    registry.register(Box::new(read_url::ReadUrlTool));
    registry.register(Box::new(view_image::ViewImageTool));
}

/// Register all built-in tools including browser tools into a ToolRegistry.
///
/// Browser tools share a `BrowserManager` instance for coordinating Chrome lifecycle.
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
