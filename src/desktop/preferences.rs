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
    pub(super) fn apply_theme(&self, window: &mut Window, cx: &mut App) {
        let dark = match self.settings.theme {
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
        next: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = next.save(&self.config_dir) {
            self.fail("설정을 저장하지 못했습니다", error, cx);
            self.sync_preferences(window, cx);
            return;
        }
        self.apply_settings(next, window, cx);
    }

    pub(super) fn reload_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match Settings::load(&self.config_dir) {
            Ok(settings) if settings != self.settings => {
                self.apply_settings(settings, window, cx);
                self.sync_preferences(window, cx);
            }
            Ok(_) => {}
            Err(error) => self.fail("설정을 읽지 못했습니다", error, cx),
        }
    }

    fn apply_settings(&mut self, next: Settings, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = next;
        self.apply_theme(window, cx);
        for tab in &self.open {
            if let Some(editor) = &tab.editor {
                editor.update(cx, |state, cx| {
                    let offset = state.scroll_offset();
                    state.set_soft_wrap(self.settings.wrap, window, cx);
                    state.set_scroll_offset(offset, cx);
                });
            }
        }
        cx.notify();
    }
    pub(super) fn sync_preferences(&self, window: &mut Window, cx: &mut App) {
        let Some(prefs) = &self.preferences else {
            return;
        };
        let mode: SharedString = match self.settings.theme {
            ThemeMode::Light => "밝게",
            ThemeMode::Dark => "어둡게",
            ThemeMode::System => "시스템",
        }
        .into();
        prefs
            .theme
            .update(cx, |s, cx| s.set_selected_value(&mode, window, cx));
        prefs.font.update(cx, |s, cx| {
            s.set_selected_value(&self.settings.font.clone().into(), window, cx)
        });
        prefs.size.update(cx, |s, cx| {
            s.set_selected_value(&self.settings.font_size.to_string().into(), window, cx)
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
        if self.preferences.is_none() {
            let mut fonts = cx.text_system().all_font_names();
            // Keep names compatible with the existing settings format and omit
            // vertical aliases, as the previous GDI enumeration did.
            fonts.retain(|name| crate::settings::valid_font(name) && !name.starts_with('@'));
            if !fonts.contains(&self.settings.font) {
                fonts.push(self.settings.font.clone());
            }
            fonts.sort_unstable();
            fonts.dedup();
            let theme = cx.new(|cx| {
                SelectState::new(
                    vec!["밝게".into(), "어둡게".into(), "시스템".into()],
                    None,
                    window,
                    cx,
                )
            });
            let font = cx.new(|cx| {
                SelectState::new(
                    fonts
                        .into_iter()
                        .map(SharedString::from)
                        .collect::<Vec<_>>(),
                    None,
                    window,
                    cx,
                )
                .searchable(true)
            });
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
                        let mut next = this.settings.clone();
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
                        if next != this.settings {
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
