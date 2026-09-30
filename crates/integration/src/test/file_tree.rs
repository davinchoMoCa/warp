use regex::Regex;
use warp::features::FeatureFlag;
use warp::integration_testing::step::new_step_with_default_assertions;
use warp::integration_testing::tab::assert_pane_title;
use warp::integration_testing::terminal::wait_until_bootstrapped_single_pane_for_tab;
use warp::integration_testing::view_getters::{pane_group_view, workspace_view};
use warp::workspace::WorkspaceAction;
use warpui_core::integration::TestStep;
use warpui_core::{App, async_assert, async_assert_eq};

use super::{Builder, new_builder};
use crate::util::write_all_rc_files_for_test;

fn open_file_tree_panel(app: &mut App) {
    let window_id = app.read(|ctx| {
        ctx.windows()
            .active_window()
            .expect("should have active window")
    });
    let workspace = workspace_view(app, window_id);
    app.update(|ctx| {
        ctx.dispatch_typed_action_for_view(
            window_id,
            workspace.id(),
            &WorkspaceAction::ToggleProjectExplorer,
        );
    });
}
pub fn test_file_tree_double_click_promotes_inactive_preview() -> Builder {
    file_tree_tabs_builder()
        .with_step(file_tree_click_step("alpha.txt", true, &["alpha.txt"], 0))
        .with_step(file_tree_click_step(
            "beta.txt",
            false,
            &["alpha.txt", "beta.txt"],
            1,
        ))
        .with_step(file_tree_click_step(
            "alpha.txt",
            false,
            &["alpha.txt", "beta.txt"],
            0,
        ))
        .with_step(file_tree_click_step(
            "beta.txt",
            true,
            &["alpha.txt", "beta.txt"],
            1,
        ))
        .with_step(file_tree_click_step(
            "gamma.txt",
            false,
            &["alpha.txt", "beta.txt", "gamma.txt"],
            2,
        ))
}

fn file_tree_tabs_builder() -> Builder {
    new_builder()
        .with_setup(|utils| {
            let test_dir = utils.test_dir();
            write_all_rc_files_for_test(&test_dir, format!("cd {}", test_dir.to_string_lossy()));
            for name in [
                "alpha.txt",
                "beta.txt",
                "gamma.txt",
                "delta.txt",
                "epsilon.txt",
                "zeta.txt",
            ] {
                std::fs::write(test_dir.join(name), name).expect("Failed to create sample file");
            }
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Enable tabbed editor and open project explorer")
                .with_action(|app, _, _| {
                    FeatureFlag::TabbedEditorView.set_enabled(true);
                    open_file_tree_panel(app);
                }),
        )
}

fn file_tree_click_step(
    name: &str,
    double_click: bool,
    expected: &[&str],
    active: usize,
) -> TestStep {
    let label = format!(
        "{} {name}",
        if double_click {
            "Double-click"
        } else {
            "Click"
        }
    );
    let position_id = format!("file_tree_item:{name}");
    let step = new_step_with_default_assertions(&label);
    let step = if double_click {
        step.with_double_click_on_saved_position(position_id)
    } else {
        step.with_click_on_saved_position(position_id)
    };
    let expected: Vec<String> = expected.iter().map(|name| (*name).to_owned()).collect();
    step.add_named_assertion(
        format!("Editor tabs after {label}"),
        move |app, window_id| {
            let pane_group = pane_group_view(app, window_id, 0);
            let (actual, active_index) = pane_group.read(app, |pane_group, ctx| {
                let (_, code_view) = pane_group.code_panes(ctx).next().expect("code editor pane");
                code_view.read(ctx, |view, _| {
                    let tabs = (0..view.tab_count())
                        .map(|index| {
                            view.tab_at(index)
                                .and_then(|tab| tab.location())
                                .and_then(|location| {
                                    std::path::Path::new(&location.display_path())
                                        .file_name()
                                        .map(|name| name.to_string_lossy().into_owned())
                                })
                                .expect("file-backed editor tab")
                        })
                        .collect::<Vec<_>>();
                    (tabs, view.active_tab_index())
                })
            });
            async_assert_eq!((actual, active_index), (expected.clone(), active))
        },
    )
}

pub fn test_file_tree_double_click_preserves_single_preview() -> Builder {
    file_tree_tabs_builder()
        .with_step(file_tree_click_step("alpha.txt", false, &["alpha.txt"], 0))
        .with_step(file_tree_click_step(
            "beta.txt",
            true,
            &["alpha.txt", "beta.txt"],
            1,
        ))
        .with_step(file_tree_click_step(
            "alpha.txt",
            false,
            &["alpha.txt", "beta.txt"],
            0,
        ))
        .with_step(file_tree_click_step(
            "alpha.txt",
            false,
            &["alpha.txt", "beta.txt"],
            0,
        ))
        .with_step(file_tree_click_step(
            "gamma.txt",
            false,
            &["gamma.txt", "beta.txt"],
            0,
        ))
        .with_step(file_tree_click_step(
            "gamma.txt",
            true,
            &["gamma.txt", "beta.txt"],
            0,
        ))
        .with_step(
            TestStep::new("Close gamma and return to one editor tab").with_action(
                |app, window_id, _| {
                    let pane_group = pane_group_view(app, window_id, 0);
                    let code_view = pane_group.read(app, |pane_group, ctx| {
                        pane_group.code_panes(ctx).next().unwrap().1
                    });
                    app.update(|ctx| {
                        code_view.update(ctx, |view, ctx| {
                            view.remove_tab_for_move(0, ctx);
                        });
                    });
                },
            ),
        )
        .with_step(file_tree_click_step(
            "delta.txt",
            false,
            &["beta.txt", "delta.txt"],
            1,
        ))
        .with_step(file_tree_click_step(
            "epsilon.txt",
            false,
            &["beta.txt", "epsilon.txt"],
            1,
        ))
}

pub fn test_file_tree_persistent_tabs_keep_order_and_evict_preview() -> Builder {
    file_tree_tabs_builder()
        .with_step(file_tree_click_step("alpha.txt", true, &["alpha.txt"], 0))
        .with_step(file_tree_click_step(
            "beta.txt",
            true,
            &["alpha.txt", "beta.txt"],
            1,
        ))
        .with_step(file_tree_click_step(
            "alpha.txt",
            false,
            &["alpha.txt", "beta.txt"],
            0,
        ))
        .with_step(file_tree_click_step(
            "gamma.txt",
            false,
            &["alpha.txt", "gamma.txt", "beta.txt"],
            1,
        ))
        .with_step(file_tree_click_step(
            "beta.txt",
            false,
            &["alpha.txt", "gamma.txt", "beta.txt"],
            2,
        ))
        .with_step(file_tree_click_step(
            "delta.txt",
            false,
            &["alpha.txt", "beta.txt", "delta.txt"],
            2,
        ))
        .with_step(file_tree_click_step(
            "alpha.txt",
            true,
            &["alpha.txt", "beta.txt", "delta.txt"],
            0,
        ))
        .with_step(file_tree_click_step(
            "epsilon.txt",
            false,
            &["alpha.txt", "epsilon.txt", "beta.txt"],
            1,
        ))
        .with_step(file_tree_click_step(
            "zeta.txt",
            true,
            &["alpha.txt", "zeta.txt", "beta.txt"],
            1,
        ))
}

/// Test that clicking a file in the file tree opens it in Warp's editor.
/// This is a regression test for the bug where files were being opened in
/// external editors instead of Warp's built-in editor.
pub fn test_file_tree_opens_files_in_warp() -> Builder {
    new_builder()
        .with_setup(|utils| {
            let test_dir = utils.test_dir();

            // Change to the test directory
            let dir_string = test_dir
                .to_str()
                .expect("Should be able to convert test dir to str");
            write_all_rc_files_for_test(&test_dir, format!("cd {dir_string}"));

            // Create a test file
            std::fs::write(test_dir.join("test_file.txt"), "Hello from test file!")
                .expect("Failed to create test file");

            // Create a test directory with a file inside
            std::fs::create_dir_all(test_dir.join("test_dir"))
                .expect("Failed to create test directory");
            std::fs::write(
                test_dir.join("test_dir/nested_file.rs"),
                "fn main() {\n    println!(\"Hello, world!\");\n}",
            )
            .expect("Failed to create nested file");
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Open file tree panel")
                .with_action(|app, _, _| open_file_tree_panel(app)),
        )
        // Click on test_file.txt in the file tree
        .with_step(
            new_step_with_default_assertions("Click on test_file.txt in file tree")
                .with_click_on_saved_position("file_tree_item:test_file.txt")
                .add_assertion(|app, window_id| {
                    // Verify that a new pane was opened with the file
                    let pane_group = pane_group_view(app, window_id, 0);
                    pane_group.read(app, |pane_group, _ctx| {
                        async_assert_eq!(
                            pane_group.pane_count(),
                            2,
                            "Expected 2 panes after opening file (terminal + editor)"
                        )
                    })
                }),
        )
        .with_step(
            new_step_with_default_assertions("Verify file opened in Warp editor").add_assertion(
                assert_pane_title(0, 1, Regex::new(r"test_file\.txt$").unwrap()),
            ),
        )
}

/// Test that the "Open in new pane" context menu action works correctly.
pub fn test_file_tree_open_in_new_pane() -> Builder {
    new_builder()
        .with_setup(|utils| {
            let test_dir = utils.test_dir();
            let dir_string = test_dir
                .to_str()
                .expect("Should be able to convert test dir to str");
            write_all_rc_files_for_test(&test_dir, format!("cd {dir_string}"));

            std::fs::write(
                test_dir.join("sample.md"),
                "# Sample Markdown\n\nThis is a test.",
            )
            .expect("Failed to create sample file");
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Open file tree panel")
                .with_action(|app, _, _| open_file_tree_panel(app)),
        )
        .with_step(
            new_step_with_default_assertions(
                "Right-click on sample.md and select 'Open in new pane'",
            )
            .with_right_click_on_saved_position("file_tree_item:sample.md")
            .with_click_on_saved_position("Open in new pane"),
        )
        .with_step(
            new_step_with_default_assertions("Verify file opened in new pane")
                .add_assertion(assert_pane_title(0, 1, Regex::new(r"sample\.md$").unwrap()))
                .add_assertion(|app, window_id| {
                    let pane_group = pane_group_view(app, window_id, 0);
                    pane_group.read(app, |pane_group, _ctx| {
                        async_assert_eq!(
                            pane_group.pane_count(),
                            2,
                            "Expected 2 panes after 'Open in new pane'"
                        )
                    })
                }),
        )
}

pub fn test_file_tree_omits_open_in_new_tab() -> Builder {
    new_builder()
        .with_setup(|utils| {
            let test_dir = utils.test_dir();
            write_all_rc_files_for_test(&test_dir, format!("cd {}", test_dir.to_string_lossy()));
            std::fs::write(test_dir.join("config.json"), "{}")
                .expect("Failed to create sample file");
            std::fs::create_dir(test_dir.join("folder"))
                .expect("Failed to create sample directory");
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Open file tree panel")
                .with_action(|app, _, _| open_file_tree_panel(app)),
        )
        .with_step(
            TestStep::new("File context menu omits new tab")
                .with_right_click_on_saved_position("file_tree_item:config.json")
                .add_named_assertion(
                    "File has new-pane but not new-tab action",
                    |app, window_id| {
                        let presenter = app.presenter(window_id).expect("window presenter");
                        let presenter = presenter.borrow();
                        let positions = presenter.position_cache();
                        async_assert!(
                            positions.get_position("Open in new pane").is_some()
                                && positions.get_position("Open in new tab").is_none(),
                            "File context menu should retain new pane but omit new tab"
                        )
                    },
                ),
        )
}

pub fn test_file_tree_directory_omits_open_in_new_tab() -> Builder {
    new_builder()
        .with_setup(|utils| {
            let test_dir = utils.test_dir();
            write_all_rc_files_for_test(&test_dir, format!("cd {}", test_dir.to_string_lossy()));
            std::fs::create_dir(test_dir.join("folder"))
                .expect("Failed to create sample directory");
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Open file tree panel")
                .with_action(|app, _, _| open_file_tree_panel(app)),
        )
        .with_step(
            TestStep::new("Directory context menu omits new tab")
                .with_right_click_on_saved_position("file_tree_item:folder")
                .add_named_assertion(
                    "Directory has new-file but not new-tab action",
                    |app, window_id| {
                        let presenter = app.presenter(window_id).expect("window presenter");
                        let presenter = presenter.borrow();
                        let positions = presenter.position_cache();
                        async_assert!(
                            positions.get_position("New file").is_some()
                                && positions.get_position("Open in new tab").is_none(),
                            "Directory context menu should retain new file but omit new tab"
                        )
                    },
                ),
        )
}

/// Test that keyboard navigation (arrow keys + enter) works to open files.
pub fn test_file_tree_keyboard_navigation() -> Builder {
    new_builder()
        .with_setup(|utils| {
            let test_dir = utils.test_dir();
            let dir_string = test_dir
                .to_str()
                .expect("Should be able to convert test dir to str");
            write_all_rc_files_for_test(&test_dir, format!("cd {dir_string}"));

            std::fs::create_dir_all(test_dir.join("src")).expect("Failed to create src directory");
            std::fs::write(test_dir.join("src/file_a.txt"), "File A")
                .expect("Failed to create file A");
            std::fs::write(test_dir.join("src/file_b.txt"), "File B")
                .expect("Failed to create file B");
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Open file tree panel")
                .with_action(|app, _, _| open_file_tree_panel(app)),
        )
        .with_step(
            new_step_with_default_assertions("Focus file tree")
                .with_click_on_saved_position("file_tree_item:src"),
        )
        .with_step(
            new_step_with_default_assertions("Navigate to a file and press Enter")
                .with_keystrokes(&["down", "enter"])
                .add_assertion(|app, window_id| {
                    let pane_group = pane_group_view(app, window_id, 0);
                    pane_group.read(app, |pane_group, _ctx| {
                        async_assert_eq!(
                            pane_group.pane_count(),
                            2,
                            "Expected 2 panes after opening file via keyboard"
                        )
                    })
                }),
        )
}

/// Test that non-text files (like images) do not crash when clicked.
/// They should either open in the system default app or show an error.
pub fn test_file_tree_non_openable_files() -> Builder {
    new_builder()
        .with_setup(|utils| {
            let test_dir = utils.test_dir();
            let dir_string = test_dir
                .to_str()
                .expect("Should be able to convert test dir to str");
            write_all_rc_files_for_test(&test_dir, format!("cd {dir_string}"));

            // Create a binary file that shouldn't be opened in Warp
            std::fs::write(test_dir.join("test.bin"), vec![0u8, 1, 2, 3, 255])
                .expect("Failed to create binary file");
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Open file tree panel")
                .with_action(|app, _, _| open_file_tree_panel(app)),
        )
        .with_step(
            new_step_with_default_assertions("Click on binary file")
                .with_click_on_saved_position("file_tree_item:test.bin")
                .add_assertion(|app, window_id| {
                    // The binary file should NOT open in a new pane in Warp
                    // It should fall back to system default behavior
                    let pane_group = pane_group_view(app, window_id, 0);
                    pane_group.read(app, |pane_group, _ctx| {
                        async_assert_eq!(
                            pane_group.pane_count(),
                            1,
                            "Binary file should not open in Warp, should stay at 1 pane"
                        )
                    })
                }),
        )
}

/// Test that expanding directories and then clicking files inside them works correctly.
pub fn test_file_tree_nested_file_opening() -> Builder {
    new_builder()
        .with_setup(|utils| {
            let test_dir = utils.test_dir();
            let dir_string = test_dir
                .to_str()
                .expect("Should be able to convert test dir to str");
            write_all_rc_files_for_test(&test_dir, format!("cd {dir_string}"));

            // Create nested directory structure
            std::fs::create_dir_all(test_dir.join("src/utils"))
                .expect("Failed to create nested directories");
            std::fs::write(
                test_dir.join("src/utils/helper.js"),
                "export function helper() { return 42; }",
            )
            .expect("Failed to create nested file");
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Open file tree panel")
                .with_action(|app, _, _| open_file_tree_panel(app)),
        )
        .with_step(
            new_step_with_default_assertions("Expand src directory")
                .with_click_on_saved_position("file_tree_item:src"),
        )
        .with_step(
            new_step_with_default_assertions("Expand utils directory")
                .with_click_on_saved_position("file_tree_item:utils"),
        )
        .with_step(
            new_step_with_default_assertions("Click on helper.js")
                .with_click_on_saved_position("file_tree_item:helper.js")
                .add_assertion(assert_pane_title(0, 1, Regex::new(r"helper\.js$").unwrap())),
        )
}
