//! The contract's single exit point.
//!
//! Every command the frontend may call is registered here, and nowhere else.
//! `src/services/bindings.ts` is generated from this file — see
//! `scripts/gen-contract.mjs`.

use tauri_specta::{collect_commands, Builder};

use crate::commands;

pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::backup::backup_save,
        commands::backup::backup_load,
        commands::node::list_nodes,
        commands::node::upsert_node,
        commands::node::delete_node,
        commands::node::move_node,
        commands::node::delete_nodes,
        commands::node::tag_nodes,
        commands::node::export_node_links,
        commands::node::test_node_latency,
        commands::settings::autostart_status,
        commands::settings::get_settings,
        commands::settings::save_settings,
        commands::system::app_info,
        commands::system::app_paths,
        commands::system::app_open_data_dir,
        commands::system::app_open_log_dir,
        commands::system::save_text_file,
        commands::system::open_text_file,
        commands::system::ui_set_window_theme,
        commands::engine::engine_status,
        commands::engine::engine_kinds,
        commands::engine::engine_binaries,
        commands::engine::engine_version,
        commands::engine::engine_open_dir,
        commands::engine::engine_log_tail,
        commands::engine::proxy_latency,
        commands::engine::proxy_speed,
        commands::engine::close_connection,
        commands::engine::engine_start,
        commands::engine::engine_stop,
        commands::engine::traffic_sample,
        commands::engine::engine_config_preview,
        commands::import::import_preview,
        commands::import::import_nodes,
        commands::group::list_groups,
        commands::group::upsert_group,
        commands::group::delete_group,
        commands::group::select_group_member,
        commands::group::toggle_group,
        commands::routing::list_rules,
        commands::routing::upsert_rule,
        commands::routing::delete_rule,
        commands::routing::move_rule,
        commands::routing::toggle_rule,
        commands::ruleset::list_rule_sets,
        commands::ruleset::create_rule_set,
        commands::ruleset::upsert_rule_set,
        commands::ruleset::delete_rule_set,
        commands::ruleset::toggle_rule_set,
        commands::ruleset::set_rule_set_text,
        commands::ruleset::refresh_rule_set,
        commands::sysproxy::system_proxy_status,
        commands::sysproxy::system_proxy_disable,
        commands::subscription::list_subscriptions,
        commands::subscription::create_subscription,
        commands::subscription::upsert_subscription,
        commands::subscription::delete_subscription,
        commands::subscription::refresh_subscription,
        commands::subscription::refresh_due,
    ])
}
