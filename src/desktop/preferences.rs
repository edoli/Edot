//! Settings persistence, font choices and theme application.
use super::*;

pub(super) struct Preferences {
    pub(super) focus: FocusHandle,
    pub(super) theme: Entity<SelectState>,
    pub(super) font: Entity<SelectState>,
    pub(super) size: Entity<SelectState>,
    pub(super) wrap_focus: FocusHandle,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl Edot {
    pub(super) fn refresh_startup_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.global::<AppState>().font_refresh_started {
            return;
        }
        cx.global_mut::<AppState>().font_refresh_started = true;
        let text_system = cx.text_system().clone();
        let refresh = cx.background_executor().spawn(async move {
            text_system
                .refresh_system_fonts_once()
                .map(|refreshed| refreshed.then(|| text_system.all_font_names()))
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = refresh.await;
            let _ = this.update_in(cx, |this, window, cx| {
                cx.global_mut::<AppState>().font_refresh = None;
                match result {
                    Ok(Some(fonts)) => this.apply_startup_font_names(fonts, window, cx),
                    Ok(None) => this.sync_fallback_font(window, cx),
                    Err(error) => {
                        eprintln!("글꼴 목록을 갱신하지 못했습니다: {error}");
                        this.sync_fallback_font(window, cx);
                    }
                }
            });
        });
        cx.global_mut::<AppState>().font_refresh = Some(task);
    }

    pub(super) fn apply_startup_font_names(
        &mut self,
        fonts: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_fallback_font(window, cx);
        if let Some(preferences) = &self.preferences {
            let choices = font_choices(fonts.clone(), &cx.global::<AppState>().settings.font);
            preferences
                .font
                .update(cx, |select, cx| select.set_items(choices, cx));
        }
        cx.global_mut::<AppState>().font_names = Some(fonts);
    }

    fn sync_fallback_font(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.startup_failed
            && let Some(font) = fallback_font(&cx.global::<AppState>().settings.font, cx)
        {
            let mut next = cx.global::<AppState>().settings.clone();
            next.font = font;
            self.change_settings(next, window, cx);
        }
    }

    pub(super) fn remember_delete_choice(
        &mut self,
        dont_ask_again: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !dont_ask_again {
            return true;
        }
        let mut next = cx.global::<AppState>().settings.clone();
        next.confirm_delete = false;
        if let Err(error) = next.save(&cx.global::<AppState>().config_dir) {
            self.fail("설정을 저장하지 못했습니다", error, cx);
            return false;
        }
        cx.global_mut::<AppState>().settings = next;
        cx.notify();
        true
    }
    pub(super) fn apply_theme(&self, window: &mut Window, cx: &mut App) {
        let dark = match cx.global::<AppState>().settings.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        Theme::update(cx, |theme| {
            theme.colors = component::theme::Colors::for_appearance(dark);
        });
    }
    pub(super) fn change_settings(
        &mut self,
        mut next: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let font_changed = next.font != cx.global::<AppState>().settings.font;
        if font_changed && let Some(font) = fallback_font(&next.font, cx) {
            next.font = font;
        }
        if let Err(error) = next.save(&cx.global::<AppState>().config_dir) {
            self.fail("설정을 저장하지 못했습니다", error, cx);
            self.sync_preferences(window, cx);
            return false;
        }
        self.apply_settings(next, window, cx);
        if font_changed {
            self.sync_preferences(window, cx);
        }
        true
    }

    pub(super) fn set_read_only(
        &mut self,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if cx.global::<AppState>().settings.read_only == read_only {
            return true;
        }
        let mut next = cx.global::<AppState>().settings.clone();
        next.read_only = read_only;
        self.change_settings(next, window, cx)
    }

    fn apply_settings(&mut self, next: Settings, window: &mut Window, cx: &mut Context<Self>) {
        cx.global_mut::<AppState>().settings = next;
        self.apply_theme(window, cx);
        for tab in &self.open {
            if let Some(editor) = &tab.editor {
                editor.update(cx, |state, cx| {
                    let offset = state.scroll_offset();
                    state.set_soft_wrap(cx.global::<AppState>().settings.wrap, window, cx);
                    state.set_scroll_offset(offset, cx);
                    state.set_read_only(cx.global::<AppState>().settings.read_only, cx);
                });
            }
        }
        cx.notify();
    }
    pub(super) fn sync_preferences(&self, window: &mut Window, cx: &mut App) {
        let Some(prefs) = &self.preferences else {
            return;
        };
        let mode: SharedString = match cx.global::<AppState>().settings.theme {
            ThemeMode::Light => "밝게",
            ThemeMode::Dark => "어둡게",
            ThemeMode::System => "시스템",
        }
        .into();
        prefs
            .theme
            .update(cx, |s, cx| s.set_selected_value(&mode, window, cx));
        prefs.font.update(cx, |s, cx| {
            s.set_selected_value(
                &cx.global::<AppState>().settings.font.clone().into(),
                window,
                cx,
            )
        });
        prefs.size.update(cx, |s, cx| {
            s.set_selected_value(
                &cx.global::<AppState>()
                    .settings
                    .font_size
                    .to_string()
                    .into(),
                window,
                cx,
            )
        });
    }
    pub(super) fn dismiss_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_visible = false;
        if let Some(prefs) = &self.preferences {
            for select in [&prefs.theme, &prefs.font, &prefs.size] {
                select.update(cx, |s, cx| s.close(cx));
            }
        }
        cx.notify();
    }
    pub(super) fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_visible = !self.settings_visible;
        if !self.settings_visible {
            self.focus_editor(window, cx);
            cx.notify();
            return;
        }
        self.find_visible = false;
        self.check_for_updates(window, cx);
        if self.preferences.is_none() {
            let fonts = cx
                .global::<AppState>()
                .font_names
                .clone()
                .unwrap_or_else(|| cx.text_system().all_font_names());
            let fonts = font_choices(fonts, &cx.global::<AppState>().settings.font);
            let theme = cx.new(|cx| {
                SelectState::new(
                    vec!["밝게".into(), "어둡게".into(), "시스템".into()],
                    None,
                    window,
                    cx,
                )
            });
            let font = cx.new(|cx| SelectState::new(fonts, None, window, cx).searchable(true));
            let size = cx.new(|cx| {
                SelectState::new(
                    (crate::settings::MIN_FONT_SIZE..=crate::settings::MAX_FONT_SIZE)
                        .map(|n| SharedString::from(n.to_string()))
                        .collect::<Vec<_>>(),
                    None,
                    window,
                    cx,
                )
                .suffix(" px")
            });
            let mut subscriptions = vec![];
            for (kind, entity) in [(0, &theme), (1, &font), (2, &size)] {
                subscriptions.push(cx.subscribe_in(
                    entity,
                    window,
                    move |this, _, event, window, cx| {
                        let SelectEvent::Confirm(Some(value)) = event else {
                            return;
                        };
                        let mut next = cx.global::<AppState>().settings.clone();
                        match kind {
                            0 => {
                                next.theme = match value.as_ref() {
                                    "밝게" => ThemeMode::Light,
                                    "어둡게" => ThemeMode::Dark,
                                    _ => ThemeMode::System,
                                }
                            }
                            1 => next.font = value.to_string(),
                            _ => {
                                next.font_size =
                                    value.parse().unwrap_or(crate::settings::DEFAULT_FONT_SIZE)
                            }
                        }
                        if next != cx.global::<AppState>().settings {
                            this.change_settings(next, window, cx);
                        }
                    },
                ));
            }
            let focus = cx.focus_handle();
            subscriptions.push(cx.on_focus_out(&focus, window, |this, _, _, cx| {
                if this.settings_visible {
                    this.dismiss_settings(cx);
                }
            }));
            subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
                if this.settings_visible && !window.is_window_active() {
                    this.dismiss_settings(cx);
                }
            }));
            self.preferences = Some(Preferences {
                focus,
                theme,
                font,
                size,
                wrap_focus: cx.focus_handle().tab_stop(true),
                _subscriptions: subscriptions,
            });
        }
        if let Some(prefs) = &self.preferences {
            for select in [&prefs.theme, &prefs.font, &prefs.size] {
                select.update(cx, |s, cx| s.close(cx));
            }
        }
        self.sync_preferences(window, cx);
        self.preferences
            .as_ref()
            .unwrap()
            .theme
            .update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }
}

fn fallback_font(family: &str, cx: &App) -> Option<String> {
    let text_system = cx.text_system();
    let requested = gpui::font(family.to_owned());
    if text_system.font_id(&requested).is_ok() {
        return None;
    }
    let resolved = text_system.resolve_font(&requested);
    text_system
        .get_font_for_id(resolved)
        .map(|font| {
            gpui::font_name_with_fallbacks(&font.family, component::theme::UI_FONT).to_owned()
        })
        .filter(|family| crate::settings::valid_font(family))
}

fn font_choices(mut fonts: Vec<String>, selected: &str) -> Vec<SharedString> {
    fonts.retain(|name| crate::settings::valid_font(name) && !name.starts_with('@'));
    if !fonts.iter().any(|name| name == selected) {
        fonts.push(selected.to_owned());
    }
    fonts.sort_unstable();
    fonts.dedup();
    fonts.into_iter().map(SharedString::from).collect()
}
