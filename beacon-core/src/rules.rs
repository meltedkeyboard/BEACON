use std::collections::HashMap;

use crate::manifest::{Rule, RuleAction};

/// Feature flags used by conditional game arguments (`is_demo_user`, `has_custom_resolution`, ...).
/// The MVP does not support any optional feature, so every lookup returns `false`, which makes
/// argument entries that require a feature get excluded, matching vanilla behavior for a plain launch.
pub struct FeatureFlags;

impl FeatureFlags {
    pub fn get(&self, _name: &str) -> bool {
        false
    }
}

fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

pub(crate) fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        std::env::consts::ARCH
    }
}

fn rule_matches(rule: &Rule, features: &FeatureFlags) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != current_os_name() {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != current_arch() {
                return false;
            }
        }
        // `os.version` is a regex matched against the OS version string in the vanilla launcher;
        // it only appears on rules targeting ancient Windows releases, irrelevant for this MVP.
    }
    if let Some(required_features) = &rule.features {
        for (name, expected) in required_features {
            if features.get(name) != *expected {
                return false;
            }
        }
    }
    true
}

/// Evaluates a rule list the same way the vanilla launcher does: no rules means always allowed,
/// otherwise the outcome starts disallowed and the last matching rule's action wins.
pub fn rules_allow(rules: &[Rule], features: &FeatureFlags) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, features) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

pub fn library_classifier_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "natives-windows"
    } else if cfg!(target_os = "macos") {
        "natives-osx"
    } else {
        "natives-linux"
    }
}

pub fn native_classifier_for(natives: &HashMap<String, String>) -> Option<String> {
    let os_key = current_os_name();
    natives
        .get(os_key)
        .map(|s| s.replace("${arch}", if cfg!(target_pointer_width = "64") { "64" } else { "32" }))
}
