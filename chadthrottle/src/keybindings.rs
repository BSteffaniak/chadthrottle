/// Centralized keybinding definitions for ChadThrottle
/// This ensures the help menu, status bar, and actual key handlers stay in sync

#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub key: &'static str,
    pub description: &'static str,
    pub category: KeyCategory,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyCategory {
    Navigation,
    Actions,
    System,
}

impl KeyCategory {
    pub fn title(&self) -> &'static str {
        match self {
            KeyCategory::Navigation => "Navigation",
            KeyCategory::Actions => "Actions",
            KeyCategory::System => "System",
        }
    }
}

/// Get all keybindings
pub fn get_all_keybindings() -> Vec<KeyBinding> {
    vec![
        // Navigation
        KeyBinding {
            key: "↑/k",
            description: "Move selection up",
            category: KeyCategory::Navigation,
        },
        KeyBinding {
            key: "↓/j",
            description: "Move selection down",
            category: KeyCategory::Navigation,
        },
        KeyBinding {
            key: "i",
            description: "Toggle interface view",
            category: KeyCategory::Navigation,
        },
        KeyBinding {
            key: "l",
            description: "Cycle traffic view (All/Internet/Local)",
            category: KeyCategory::Navigation,
        },
        KeyBinding {
            key: "Enter",
            description: "View interface details (in interface list)",
            category: KeyCategory::Navigation,
        },
        KeyBinding {
            key: "Space",
            description: "Toggle interface filter (in interface list)",
            category: KeyCategory::Navigation,
        },
        KeyBinding {
            key: "A",
            description: "Toggle All/None interfaces (in interface list)",
            category: KeyCategory::Navigation,
        },
        // Actions
        KeyBinding {
            key: "t",
            description: "Throttle selected process",
            category: KeyCategory::Actions,
        },
        KeyBinding {
            key: "r",
            description: "Remove throttle",
            category: KeyCategory::Actions,
        },
        KeyBinding {
            key: "g",
            description: "Toggle bandwidth graph",
            category: KeyCategory::Actions,
        },
        KeyBinding {
            key: "f",
            description: "Freeze/unfreeze sort order",
            category: KeyCategory::Actions,
        },
        // System
        KeyBinding {
            key: "b",
            description: "View/switch backends",
            category: KeyCategory::System,
        },
        KeyBinding {
            key: "h/?",
            description: "Toggle this help",
            category: KeyCategory::System,
        },
        KeyBinding {
            key: "q/Esc",
            description: "Quit (or close modal if open)",
            category: KeyCategory::System,
        },
        KeyBinding {
            key: "Ctrl+C",
            description: "Force quit (always exits)",
            category: KeyCategory::System,
        },
    ]
}

/// Get keybindings for the status bar (most common ones)
pub fn get_status_bar_keybindings() -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑↓", "Navigate"),
        ("i", "Interfaces"),
        ("l", "Traffic"),
        ("t", "Throttle"),
        ("f", "Freeze"),
        ("b", "Backends"),
        ("h", "Help"),
        ("q/Ctrl+C", "Quit"),
    ]
}
