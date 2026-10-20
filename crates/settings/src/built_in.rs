use settings_content::*;

pub const DEFAULT_WINDOW_TITLE_FORMAT: &str = "${projectName}${separator}${fileName}";

/// The complete application settings. Edit this constructor to change persistent preferences.
pub fn settings() -> SettingsContent {
    SettingsContent {
        project: project_settings(),
        theme: theme_settings(),
        workspace: workspace_settings(),
        editor: editor_settings(),
        remote: RemoteSettingsContent {
            ssh_connections: Some(vec![]),
            read_ssh_config: Some(true),
        },
        command_palette: Some(CommandPaletteSettingsContent {
            use_command_history: Some(true),
        }),
        file_finder: Some(FileFinderSettingsContent {
            file_icons: Some(true),
            modal_max_width: Some(ModalWidthContent::Large),
            skip_focus_for_active_in_search: Some(true),
            include_ignored: Some(IncludeIgnoredContent::Smart),
            prefill_query_from_selection: Some(true),
        }),
        call_hierarchy: Some(CallHierarchySettingsContent {
            modal_max_width: Some(ModalWidthContent::Medium),
        }),
        git_panel: Some(GitPanelSettingsContent {
            button: Some(true),
            dock: Some(DockPosition::Left),
            default_width: Some(PixelSetting(360.0)),
            status_style: Some(StatusStyle::Icon),
            file_icons: Some(false),
            folder_indicator: Some(FolderIndicator::Icon),
            scrollbar: Some(ScrollbarSettings {
                ..Default::default()
            }),
            fallback_branch_name: Some("main".into()),
            sort_by: Some(GitPanelSortBy::Path),
            group_by: Some(GitPanelGroupBy::Status),
            collapse_untracked_diff: Some(false),
            tree_view: Some(true),
            diff_stats: Some(true),
            show_count_badge: Some(false),
            starts_open: Some(false),
            commit_title_max_length: Some(0),
            entry_primary_click_action: Some(GitPanelClickBehavior::ProjectDiff),
            commit_editor: Some(GitPanelCommitEditor::Expanded),
        }),
        tabs: Some(ItemSettingsContent {
            git_status: Some(true),
            close_position: Some(ClosePosition::Right),
            file_icons: Some(true),
            activate_on_close: Some(ActivateOnClose::History),
            show_diagnostics: Some(ShowDiagnostics::Off),
            show_close_button: Some(ShowCloseButton::Hover),
        }),
        tab_bar: Some(TabBarSettingsContent {
            show: Some(true),
            show_nav_history_buttons: Some(true),
            show_tab_bar_buttons: Some(true),
            show_pinned_tabs_in_separate_row: Some(false),
        }),
        tab_switcher: Some(TabSwitcherSettingsContent {
            show_file_preview: Some(true),
        }),
        status_bar: Some(StatusBarSettingsContent {
            show: Some(true),
            show_active_file: Some(false),
            active_language_button: Some(true),
            cursor_position_button: Some(true),
            line_endings_button: Some(false),
            active_encoding_button: Some(EncodingDisplayOptions::NonUtf8),
            pending_keystrokes_indicator: Some(true),
        }),
        preview_tabs: Some(PreviewTabsSettingsContent {
            enabled: Some(true),
            enable_preview_from_project_panel: Some(true),
            enable_preview_from_file_finder: Some(false),
            enable_preview_from_multibuffer: Some(true),
            enable_preview_multibuffer_from_code_navigation: Some(false),
            enable_preview_file_from_code_navigation: Some(true),
            enable_keep_preview_on_code_navigation: Some(false),
        }),
        diagnostics: Some(DiagnosticsSettingsContent {
            button: Some(true),
            include_warnings: Some(true),
            lsp_pull_diagnostics: Some(LspPullDiagnosticsSettingsContent {
                enabled: Some(true),
                debounce_ms: Some(DelayMs(50)),
            }),
            inline: Some(InlineDiagnosticsSettingsContent {
                enabled: Some(true),
                update_debounce_ms: Some(DelayMs(150)),
                padding: Some(4),
                min_column: Some(0),
                max_severity: Some(DiagnosticSeverityContent::Error),
            }),
        }),
        git: Some(GitSettings {
            enabled: Some(GitEnabledSettings {
                disable_git: Some(false),
                enable_status: Some(true),
                enable_diff: Some(true),
            }),
            git_gutter: Some(GitGutterSetting::TrackedFiles),
            gutter_debounce: Some(0),
            inline_blame: Some(InlineBlameSettings {
                enabled: Some(true),
                delay_ms: Some(DelayMs(0)),
                location: Some(InlineBlameLocation::StatusBar),
                padding: Some(7),
                min_column: Some(0),
                show_commit_summary: Some(false),
            }),
            branch_picker: Some(BranchPickerSettingsContent {
                show_author_name: Some(true),
            }),
            file_diff: Some(FileDiffSettingsContent {
                show_full_file: Some(true),
            }),
            hunk_style: Some(GitHunkStyleSetting::StagedHollow),
            diff_base: Some(GitDiffBaseSetting::Head),
            path_style: Some(GitPathStyle::FileNameFirst),
            show_stage_restore_buttons: Some(true),
            worktree_directory: Some("../worktrees".into()),
        }),
        global_lsp_settings: Some(GlobalLspSettingsContent {
            button: Some(true),
            request_timeout: Some(120),
            max_buffer_line_length: Some(20000),
            notifications: Some(LspNotificationSettingsContent {
                dismiss_timeout_ms: Some(5000),
            }),
            semantic_token_rules: Some(SemanticTokenRules {
                rules: vec![
                    SemanticTokenRule {
                        token_type: Some("enumMember".into()),
                        style: vec!["type".into()],
                        ..Default::default()
                    },
                    SemanticTokenRule {
                        token_type: Some("namespace".into()),
                        style: vec!["variable".into()],
                        ..Default::default()
                    },
                ],
            }),
        }),
        image_viewer: Some(ImageViewerSettingsContent {
            unit: Some(ImageFileSizeUnit::Binary),
        }),
        markdown_preview: Some(MarkdownPreviewSettingsContent {
            font_family: None,
            code_font_family: None,
            font_size: None,
            heading_font_weight: Some(FontWeightContent(600.0)),
            open_markdown_files_in_preview: Some(false),
            limit_content_width: Some(true),
            max_width: Some(PixelSetting(1200.0)),
            ..Default::default()
        }),
        hide_mouse: Some(HideMouseMode::OnTypingAndAction),
        log: Some(Default::default()),
        line_indicator_format: Some(LineIndicatorFormat::Long),
        outline_panel: Some(OutlinePanelSettingsContent {
            button: Some(true),
            default_width: Some(PixelSetting(300.0)),
            dock: Some(DockSide::Right),
            file_icons: Some(true),
            folder_indicator: Some(FolderIndicator::Icon),
            git_status: Some(true),
            indent_size: Some(PixelSetting(20.0)),
            auto_reveal_entries: Some(true),
            auto_fold_dirs: Some(true),
            indent_guides: Some(IndentGuidesSettingsContent {
                show: Some(ShowIndentGuides::Always),
            }),
            scrollbar: Some(ScrollbarSettingsContent { show: None }),
            expand_outlines_with_depth: Some(100),
            multi_buffer_hide_symbols: Some(false),
        }),
        project_panel: Some(ProjectPanelSettingsContent {
            button: Some(true),
            hide_gitignore: Some(false),
            default_width: Some(PixelSetting(240.0)),
            dock: Some(DockSide::Left),
            title_tooltip_delay: Some(ProjectPanelTitleTooltipDelay::Default),
            entry_spacing: Some(ProjectPanelEntrySpacing::Comfortable),
            file_icons: Some(true),
            folder_indicator: Some(FolderIndicator::Icon),
            git_status: Some(true),
            indent_size: Some(PixelSetting(20.0)),
            auto_reveal_entries: Some(true),
            auto_fold_dirs: Some(true),
            bold_folder_labels: Some(false),
            starts_open: Some(true),
            scrollbar: Some(ProjectPanelScrollbarSettingsContent {
                show: None,
                horizontal_scroll: Some(true),
            }),
            show_diagnostics: Some(ShowDiagnostics::All),
            indent_guides: Some(ProjectPanelIndentGuidesSettings {
                show: Some(ShowIndentGuides::Always),
            }),
            hide_root: Some(false),
            hide_hidden: Some(false),
            sticky_scroll: Some(true),
            drag_and_drop: Some(true),
            auto_open: Some(ProjectPanelAutoOpenSettings {
                on_create: Some(true),
                on_paste: Some(true),
                on_drop: Some(true),
            }),
            sort_mode: Some(ProjectPanelSortMode::DirectoriesFirst),
            sort_order: Some(ProjectPanelSortOrder::Default),
            diagnostic_badges: Some(true),
            git_status_indicator: Some(false),
        }),
        reduce_motion: Some(ReduceMotionMode::Off),
        session: Some(SessionSettingsContent {
            restore_unsaved_buffers: Some(true),
            trust_all_worktrees: Some(false),
        }),
        terminal: terminal_settings(),
        title_bar: Some(TitleBarSettingsContent {
            show_branch_status_icon: Some(true),
            show_branch_name: Some(true),
            show_worktree_name: Some(true),
            show_project_items: Some(true),
            show_menus: Some(false),
            open_menus_on_hover: Some(false),
        }),
        modeline_lines: Some(5),
    }
}

fn terminal_settings() -> Option<TerminalSettingsContent> {
    Some(TerminalSettingsContent {
        project: ProjectTerminalSettingsContent {
            shell: Some(Shell::System),
            working_directory: Some(WorkingDirectory::CurrentProjectDirectory),
            env: Some(Default::default()),
            detect_venv: Some(true),
            path_hyperlink_regexes: Some(
                PathHyperlinkRegexes(
                    vec![
                        PathHyperlinkRegex::SingleLine("File \"(?<path>[^\"]+)\", line (?<line>[0-9]+)"
                        .into()), PathHyperlinkRegex::MultiLine(vec!["(?x)".into(),
                        "(?<path>".into(), "    (".into(),
                        "        # multi-char path: first char (not opening delimiter, space, or box drawing char)"
                        .into(), "        [^({\\[<\"'`\\ \\u2500-\\u257F]".into(),
                        "        # middle chars: non-space, and colon/paren only if not followed by digit/paren/space"
                        .into(), "        ([^\\ :(]|[:(][^0-9()\\ ])*".into(),
                        "        # last char: not closing delimiter or colon".into(),
                        "        [^()}\\]>\"'`.,;:\\ ]".into(), "    |".into(),
                        "        # single-char path: not delimiter, punctuation, space, or box drawing char"
                        .into(), "        [^(){}\\[\\]<>\"'`.,;:\\ \\u2500-\\u257F]"
                        .into(), "    )".into(),
                        "    # optional line/column suffix (included in path for PathWithPosition::parse_str)"
                        .into(), "    (:+[0-9]+(:[0-9]+)?|:?\\([0-9]+([,:]?[0-9]+)?\\))?"
                        .into(), ")".into()])
                    ],
                ),
            ),
            path_hyperlink_timeout_ms: Some(1),
        },
        line_height: Some(TerminalLineHeight::Comfortable),
        font_weight: Some(FontWeightContent(400.0)),
        cursor_shape: Some(CursorShapeContent::Bar),
        blinking: Some(TerminalBlink::On),
        alternate_scroll: Some(AlternateScroll::On),
        option_as_meta: Some(true),
        copy_on_select: Some(true),
        keep_selection_on_copy: Some(true),
        open_links_in_mouse_mode: Some(true),
        button: Some(true),
        dock: Some(TerminalDockPosition::Bottom),
        starts_open: Some(false),
        flexible: Some(true),
        default_width: Some(PixelSetting(640.0)),
        default_height: Some(PixelSetting(320.0)),
        max_scroll_history_lines: Some(10000),
        scroll_multiplier: Some(1.0),
        toolbar: Some(TerminalToolbarContent {
            breadcrumbs: Some(false),
        }),
        scrollbar: Some(ScrollbarSettingsContent {
            show: None,
        }),
        minimum_contrast: Some(45.0),
        show_count_badge: Some(false),
        bell: Some(TerminalBell::Off),
        ..Default::default()
    })
}

fn editor_settings() -> EditorSettingsContent {
    EditorSettingsContent {
        cursor_blink: Some(true),
        cursor_shape: Some(CursorShape::Bar),
        cursor_animation: Some(CursorAnimationSettingsContent {
            enabled: Some(true),
        }),
        snippet_sort_order: Some(SnippetSortOrder::Inline),
        current_line_highlight: Some(CurrentLineHighlight::All),
        selection_highlight: Some(true),
        rounded_selection: Some(true),
        lsp_highlight_debounce: Some(DelayMs(75)),
        hover_popover_enabled: Some(true),
        hover_popover_delay: Some(DelayMs(300)),
        hover_popover_sticky: Some(true),
        hover_popover_hiding_delay: Some(DelayMs(300)),
        toolbar: Some(ToolbarContent {
            breadcrumbs: Some(true),
            quick_actions: Some(true),
            selections_menu: Some(true),
            code_actions: Some(false),
        }),
        scrollbar: Some(ScrollbarContent {
            show: Some(ShowScrollbar::Auto),
            git_diff: Some(true),
            search_results: Some(true),
            selected_text: Some(true),
            selected_symbol: Some(true),
            diagnostics: Some(ScrollbarDiagnostics::All),
            cursors: Some(true),
            axes: Some(ScrollbarAxesContent {
                horizontal: Some(true),
                vertical: Some(true),
            }),
        }),
        minimap: Some(MinimapContent {
            show: Some(ShowMinimap::Always),
            display_in: Some(DisplayIn::ActiveEditor),
            thumb: Some(MinimapThumb::Always),
            thumb_border: Some(MinimapThumbBorder::None),
            current_line_highlight: None,
            max_width_columns: Some(std::num::NonZeroU32::new(80).unwrap()),
        }),
        gutter: Some(GutterContent {
            line_numbers: Some(true),
            min_line_number_digits: Some(4),
            runnables: Some(true),
            folds: Some(true),
            git_gutter_width: Some(GitGutterWidth::Default),
        }),
        scroll_beyond_last_line: Some(ScrollBeyondLastLine::OnePage),
        vertical_scroll_margin: Some(3.0),
        autoscroll_on_clicks: Some(false),
        horizontal_scroll_margin: Some(5.0),
        scroll_sensitivity: Some(1.0),
        mouse_wheel_zoom: Some(false),
        fast_scroll_sensitivity: Some(4.0),
        sticky_scroll: Some(StickyScrollContent {
            enabled: Some(false),
        }),
        relative_line_numbers: Some(RelativeLineNumbers::Disabled),
        seed_search_query_from_cursor: Some(SeedQuerySetting::Always),
        use_smartcase_search: Some(false),
        multi_cursor_modifier: Some(MultiCursorModifier::Alt),
        redact_private_values: Some(false),
        expand_excerpt_lines: Some(5),
        excerpt_context_lines: Some(5),
        middle_click_paste: Some(true),
        double_click_in_multibuffer: Some(DoubleClickInMultibuffer::Select),
        search_wrap: Some(true),
        search: Some(SearchSettingsContent {
            button: Some(true),
            whole_word: Some(false),
            case_sensitive: Some(false),
            include_ignored: Some(false),
            regex: Some(false),
            center_on_match: Some(false),
            search_on_type: Some(true),
        }),
        auto_signature_help: Some(false),
        language_detection: Some(true),
        show_signature_help_after_edits: Some(false),
        minimum_contrast_for_highlights: Some(MinimumContrast(45.0)),
        go_to_definition_fallback: Some(GoToDefinitionFallback::FindAllReferences),
        lsp_results_location: Some(OpenResultsIn::MultiBuffer),
        go_to_definition_scroll_strategy: Some(GoToDefinitionScrollStrategy::Center),
        diagnostics_max_severity: Some(DiagnosticSeverityContent::All),
        inline_code_actions: Some(true),
        drag_and_drop_selection: Some(DragAndDropSelectionContent {
            enabled: Some(true),
            delay: Some(DelayMs(300)),
        }),
        code_lens: Some(CodeLens::Off),
        lsp_document_colors: Some(DocumentColorsRenderMode::Inlay),
        lsp_document_links: Some(true),
        completion_menu_scrollbar: Some(ShowScrollbar::Never),
        completion_detail_alignment: Some(CompletionDetailAlignment::Left),
        completion_menu_item_kind: Some(CompletionMenuItemKind::Symbol),
        diff_view_style: Some(DiffViewStyle::Split),
        minimum_split_diff_width: Some(100.0),
    }
}

fn workspace_settings() -> WorkspaceSettingsContent {
    WorkspaceSettingsContent {
        active_pane_modifiers: Some(ActivePaneModifiers {
            border_size: Some(PixelSetting(0.0)),
            inactive_opacity: Some(InactiveOpacity(0.7)),
        }),
        text_rendering_mode: Some(TextRenderingMode::PlatformDefault),
        bottom_dock_layout: Some(BottomDockLayout::Contained),
        pane_split_direction_horizontal: Some(PaneSplitDirectionHorizontal::Down),
        pane_split_direction_vertical: Some(PaneSplitDirectionVertical::Right),
        centered_layout: Some(CenteredLayoutSettings {
            left_padding: Some(CenteredPaddingSettings(0.2)),
            right_padding: Some(CenteredPaddingSettings(0.2)),
        }),
        confirm_quit: Some(false),
        autosave: Some(AutosaveSetting::OnFocusChange),
        restore_on_startup: Some(RestoreOnStartupBehavior::LastSession),
        cli_default_open_behavior: Some(CliDefaultOpenBehavior::ExistingWindow),
        default_open_behavior: Some(DefaultOpenBehavior::ExistingWindow),
        restore_on_file_reopen: Some(true),
        reveal_if_open: Some(false),
        drop_target_size: Some(0.2),
        when_closing_with_no_tabs: Some(CloseWindowWhenNoItems::PlatformDefault),
        accessible_mode: Some(false),
        use_system_path_prompts: Some(true),
        command_aliases: Default::default(),
        max_tabs: None,
        on_new_window: Some(OnNewWindow::Launchpad),
        resize_all_panels_in_dock: Some(vec![DockPosition::Left]),
        close_on_file_delete: Some(false),
        fullscreen_mode: Some(FullscreenMode::Native),
        close_panel_on_toggle: Some(false),
        window_title_format: Some(DEFAULT_WINDOW_TITLE_FORMAT.into()),
        window_title_separator: Some(" — ".into()),
        window_decorations: Some(WindowDecorations::Client),
        focus_follows_mouse: Some(FocusFollowsMouse {
            enabled: Some(false),
            debounce_ms: Some(250),
        }),
    }
}

fn theme_settings() -> Box<ThemeSettingsContent> {
    Box::new(ThemeSettingsContent {
        ui_font_size: Some(FontSize(16.0)),
        ui_font_family: Some(FontFamilyName(".SystemUIFont".into())),
        ui_font_fallbacks: None,
        ui_font_features: Some(FontFeaturesContent(
            [("calt".into(), 0)].into_iter().collect(),
        )),
        ui_font_weight: Some(FontWeightContent(400.0)),
        buffer_font_family: Some(FontFamilyName("JetBrainsMono Nerd Font".into())),
        buffer_font_fallbacks: None,
        buffer_font_size: Some(FontSize(15.0)),
        buffer_font_weight: Some(FontWeightContent(400.0)),
        buffer_line_height: Some(BufferLineHeight::Comfortable),
        buffer_font_features: Some(FontFeaturesContent(Default::default())),
        git_commit_buffer_font_size: Some(FontSize(12.0)),
        mermaid_font_family: None,
        theme: Some(ThemeSelection::Static(ThemeName("Nord".into()))),
        icon_theme: Some(IconThemeSelection::Static(IconThemeName(
            "Zed (Default)".into(),
        ))),
        unnecessary_code_fade: Some(CodeFade(0.3)),
        ..Default::default()
    })
}

fn project_settings() -> ProjectSettingsContent {
    ProjectSettingsContent {
        all_languages: language_settings(),
        worktree: WorktreeSettingsContent {
            file_scan_exclusions: Some(SplicingVec(vec![
                "**/.git".into(),
                "**/.svn".into(),
                "**/.hg".into(),
                "**/.jj".into(),
                "**/.sl".into(),
                "**/.repo".into(),
                "**/CVS".into(),
                "**/.DS_Store".into(),
                "**/Thumbs.db".into(),
                "**/.classpath".into(),
                "**/.settings".into(),
            ])),
            file_scan_inclusions: Some(SplicingVec(vec![".env*".into()])),
            scan_symlinks: Some(ScanSymlinksSetting::Expanded),
            file_scan_depth: Some(5),
            private_files: Some(ExtendingVec(vec![
                "**/.env*".into(),
                "**/*.pem".into(),
                "**/*.key".into(),
                "**/*.cert".into(),
                "**/*.crt".into(),
                "**/secrets.yml".into(),
            ])),
            hidden_files: Some(SplicingVec(vec!["**/.*".into()])),
            read_only_files: Some(SplicingVec(vec![])),
        },
        lsp: LspSettingsMap(
            [(
                "rust-analyzer".into(),
                LspSettings {
                    initialization_options: Some(serde_json::json!({
                        "rustfmt": { "extraArgs": ["+nightly"] },
                        "check": { "command": "clippy" },
                        "cargo": { "features": "all" },
                    })),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        ),
    }
}

fn language_settings() -> AllLanguageSettingsContent {
    AllLanguageSettingsContent {
        defaults: LanguageSettingsContent {
            tab_size: Some(std::num::NonZeroU32::new(4).unwrap()),
            hard_tabs: Some(false),
            soft_wrap: Some(SoftWrap::None),
            soft_wrap_indent: Some(SoftWrapIndent::Same),
            preferred_line_length: Some(120),
            show_wrap_guides: Some(true),
            wrap_guides: Some(vec![120]),
            indent_guides: Some(IndentGuideSettingsContent {
                enabled: Some(true),
                line_width: Some(1),
                active_line_width: Some(1),
                coloring: Some(IndentGuideColoring::IndentAware),
                background_coloring: Some(IndentGuideBackgroundColoring::Disabled),
            }),
            format_on_save: Some(FormatOnSave::On),
            remove_trailing_whitespace_on_save: Some(true),
            ensure_final_newline_on_save: Some(true),
            line_ending: Some(LineEndingSetting::Detect),
            formatter: Some(FormatterList::Single(Formatter::Auto)),
            jsx_tag_auto_close: Some(JsxTagAutoCloseSettingsContent {
                enabled: Some(true),
            }),
            enable_language_server: Some(true),
            language_servers: Some(vec![ConfiguredLanguageServer::from("...")]),
            semantic_tokens: Some(SemanticTokens::Combined),
            document_folding_ranges: Some(DocumentFoldingRanges::Off),
            document_symbols: Some(DocumentSymbols::On),
            allow_rewrap: Some(RewrapBehavior::InComments),
            show_whitespaces: Some(ShowWhitespaceSetting::Selection),
            whitespace_map: Some(WhitespaceMapContent {
                space: Some('•'),
                tab: Some('→'),
            }),
            extend_comment_on_newline: Some(true),
            extend_list_on_newline: Some(true),
            indent_list_on_tab: Some(true),
            inlay_hints: Some(InlayHintSettingsContent {
                enabled: Some(true),
                show_type_hints: Some(true),
                show_parameter_hints: Some(true),
                show_other_hints: Some(true),
                show_background: Some(false),
                edit_debounce_ms: Some(700),
                scroll_debounce_ms: Some(50),
                toggle_on_modifiers_press: Some(ModifiersContent {
                    control: false,
                    alt: false,
                    shift: false,
                    platform: false,
                    function: false,
                }),
            }),
            use_autoclose: Some(true),
            use_auto_surround: Some(true),
            always_treat_brackets_as_autoclosed: Some(false),
            use_on_type_format: Some(true),
            code_actions_on_format: Some(Default::default()),
            linked_edits: Some(true),
            auto_indent: Some(AutoIndentMode::SyntaxAware),
            auto_indent_on_paste: Some(true),
            tasks: Some(LanguageTaskSettingsContent {
                variables: Some(Default::default()),
                enabled: Some(true),
                prefer_lsp: Some(true),
            }),
            show_completions_on_input: Some(true),
            show_completion_documentation: Some(true),
            completions: Some(CompletionSettingsContent {
                words: Some(WordsCompletionMode::Fallback),
                words_min_length: Some(3),
                lsp: Some(true),
                lsp_fetch_timeout_ms: Some(0),
                lsp_insert_mode: Some(LspInsertMode::ReplaceSuffix),
            }),
            word_diff_enabled: Some(true),
            colorize_brackets: Some(false),
        },
        languages: language_overrides(),
        file_types: Some(FileTypeMap(
            [
                (
                    "JSONC".into(),
                    ExtendingSet(
                        [
                            "**/.zed/*.json".into(),
                            "**/.vscode/**/*.json".into(),
                            "**/{zed,Zed}/{settings,keymap,tasks,debug}.json".into(),
                            "tsconfig*.json".into(),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ),
                (
                    "Markdown".into(),
                    ExtendingSet(
                        [
                            ".rules".into(),
                            ".cursorrules".into(),
                            ".windsurfrules".into(),
                            ".clinerules".into(),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ),
                (
                    "Env".into(),
                    ExtendingSet([".env.*".into()].into_iter().collect()),
                ),
            ]
            .into_iter()
            .collect(),
        )),
    }
}

fn language_overrides() -> LanguageToSettingsMap {
    LanguageToSettingsMap(
        [
            (
                "Astro".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("astro-language-server"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "C".into(),
                LanguageSettingsContent {
                    use_on_type_format: Some(false),
                    ..Default::default()
                },
            ),
            (
                "C++".into(),
                LanguageSettingsContent {
                    use_on_type_format: Some(false),
                    ..Default::default()
                },
            ),
            (
                "CSharp".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("roslyn"),
                        ConfiguredLanguageServer::from("!csharp-ls"),
                        ConfiguredLanguageServer::from("!omnisharp"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Dart".into(),
                LanguageSettingsContent {
                    tab_size: Some(std::num::NonZeroU32::new(2).unwrap()),
                    ..Default::default()
                },
            ),
            (
                "Diff".into(),
                LanguageSettingsContent {
                    remove_trailing_whitespace_on_save: Some(false),
                    ensure_final_newline_on_save: Some(false),
                    ..Default::default()
                },
            ),
            (
                "EEx".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("elixir-ls"),
                        ConfiguredLanguageServer::from("!expert"),
                        ConfiguredLanguageServer::from("!dexter"),
                        ConfiguredLanguageServer::from("!next-ls"),
                        ConfiguredLanguageServer::from("!lexical"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Elixir".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("elixir-ls"),
                        ConfiguredLanguageServer::from("!expert"),
                        ConfiguredLanguageServer::from("!dexter"),
                        ConfiguredLanguageServer::from("!next-ls"),
                        ConfiguredLanguageServer::from("!lexical"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Elm".into(),
                LanguageSettingsContent {
                    tab_size: Some(std::num::NonZeroU32::new(4).unwrap()),
                    ..Default::default()
                },
            ),
            (
                "Erlang".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("erlang-ls"),
                        ConfiguredLanguageServer::from("!elp"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Git Commit".into(),
                LanguageSettingsContent {
                    soft_wrap: Some(SoftWrap::EditorWidth),
                    allow_rewrap: Some(RewrapBehavior::Anywhere),
                    ..Default::default()
                },
            ),
            (
                "Go".into(),
                LanguageSettingsContent {
                    hard_tabs: Some(true),
                    code_actions_on_format: Some(
                        [("source.organizeImports".into(), true)]
                            .into_iter()
                            .collect(),
                    ),
                    ..Default::default()
                },
            ),
            (
                "HEEx".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("elixir-ls"),
                        ConfiguredLanguageServer::from("!expert"),
                        ConfiguredLanguageServer::from("!dexter"),
                        ConfiguredLanguageServer::from("!next-ls"),
                        ConfiguredLanguageServer::from("!lexical"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "HTML+ERB".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("herb"),
                        ConfiguredLanguageServer::from("!ruby-lsp"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "JSON".into(),
                LanguageSettingsContent {
                    preferred_line_length: Some(120),
                    ..Default::default()
                },
            ),
            (
                "JS+ERB".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("!ruby-lsp"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Kotlin".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("!kotlin-language-server"),
                        ConfiguredLanguageServer::from("kotlin-lsp"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "LaTeX".into(),
                LanguageSettingsContent {
                    formatter: Some(FormatterList::Single(Formatter::LanguageServer(
                        Default::default(),
                    ))),
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("texlab"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Markdown".into(),
                LanguageSettingsContent {
                    soft_wrap: Some(SoftWrap::EditorWidth),
                    preferred_line_length: Some(300),
                    remove_trailing_whitespace_on_save: Some(false),
                    allow_rewrap: Some(RewrapBehavior::Anywhere),
                    use_on_type_format: Some(false),
                    completions: Some(CompletionSettingsContent {
                        words: Some(WordsCompletionMode::Disabled),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            (
                "PHP".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("phpactor"),
                        ConfiguredLanguageServer::from("!intelephense"),
                        ConfiguredLanguageServer::from("!phptools"),
                        ConfiguredLanguageServer::from("!phpantom"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Plain Text".into(),
                LanguageSettingsContent {
                    soft_wrap: Some(SoftWrap::EditorWidth),
                    allow_rewrap: Some(RewrapBehavior::Anywhere),
                    completions: Some(CompletionSettingsContent {
                        words: Some(WordsCompletionMode::Disabled),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            (
                "Proto".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("buf"),
                        ConfiguredLanguageServer::from("!protols"),
                        ConfiguredLanguageServer::from("!protobuf-language-server"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Python".into(),
                LanguageSettingsContent {
                    formatter: Some(FormatterList::Single(Formatter::LanguageServer(
                        LanguageServerFormatterSpecifier::Specific {
                            name: "ruff".into(),
                        },
                    ))),
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("ty"),
                        ConfiguredLanguageServer::from("ruff"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    code_actions_on_format: Some(
                        [("source.organizeImports.ruff".into(), true)]
                            .into_iter()
                            .collect(),
                    ),
                    ..Default::default()
                },
            ),
            (
                "Ruby".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("solargraph"),
                        ConfiguredLanguageServer::from("!ruby-lsp"),
                        ConfiguredLanguageServer::from("!rubocop"),
                        ConfiguredLanguageServer::from("!sorbet"),
                        ConfiguredLanguageServer::from("!steep"),
                        ConfiguredLanguageServer::from("!kanayago"),
                        ConfiguredLanguageServer::from("!fuzzy-ruby-server"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Rust".into(),
                LanguageSettingsContent {
                    ..Default::default()
                },
            ),
            (
                "Starlark".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("starpls"),
                        ConfiguredLanguageServer::from("!buck2-lsp"),
                        ConfiguredLanguageServer::from("!tilt"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Svelte".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("svelte-language-server"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Typst".into(),
                LanguageSettingsContent {
                    allow_rewrap: Some(RewrapBehavior::Anywhere),
                    ..Default::default()
                },
            ),
            (
                "SystemVerilog".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("slang"),
                        ConfiguredLanguageServer::from("!verible"),
                        ConfiguredLanguageServer::from("!veridian"),
                        ConfiguredLanguageServer::from("!svls"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    use_on_type_format: Some(false),
                    ..Default::default()
                },
            ),
            (
                "YAML+ERB".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("!ruby-lsp"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Zig".into(),
                LanguageSettingsContent {
                    language_servers: Some(vec![
                        ConfiguredLanguageServer::from("zls"),
                        ConfiguredLanguageServer::from("..."),
                    ]),
                    ..Default::default()
                },
            ),
        ]
        .into_iter()
        .collect(),
    )
}
