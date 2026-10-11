//! Settings-only release checks and safe installer handoff.
use super::*;
use crate::update::{self, State};
use std::time::{Duration, Instant};

impl Edot {
    pub(super) fn check_for_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.global::<AppState>();
        // Headless tests cannot wake the scheduler from a native worker thread.
        if cfg!(test)
            || matches!(state.update_state, State::Checking | State::Installing)
            || state
                .update_checked
                .is_some_and(|time| time.elapsed() < Duration::from_secs(300))
        {
            return;
        }
        cx.global_mut::<AppState>().update_state = State::Checking;
        let request = self.update_worker(cx).spawn(async { update::check() });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |_, _, cx| {
                let state = cx.global_mut::<AppState>();
                state.update_checked = Some(Instant::now());
                state.update_state =
                    result.unwrap_or_else(|error| State::Failed(error.to_string()));
                state.update_task = None;
                cx.notify();
            });
        });
        cx.global_mut::<AppState>().update_task = Some(task);
        cx.notify();
    }

    fn update_worker<'a>(&self, cx: &'a mut Context<Self>) -> &'a DedicatedExecutor {
        if cx.global::<AppState>().update_worker.is_none() {
            let worker = DedicatedExecutor::new(&cx.background_executor().scheduler_executor());
            cx.global_mut::<AppState>().update_worker = Some(worker);
        }
        cx.global::<AppState>().update_worker.as_ref().unwrap()
    }

    pub(super) fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Available(release) = cx.global::<AppState>().update_state.clone() else {
            return;
        };
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                cx.global_mut::<AppState>().update_message = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(scope) = update::install_scope(&exe) else {
            cx.open_url(&release.page_url());
            return;
        };
        if self.startup_failed {
            cx.global_mut::<AppState>().update_message =
                Some("Space를 정상적으로 연 뒤 업데이트하세요.".into());
            cx.notify();
            return;
        }
        let origin_space = self.dir.clone();
        let space = origin_space.clone();
        let config = cx.global::<AppState>().config_dir.clone();
        cx.global_mut::<AppState>().update_state = State::Installing;
        cx.global_mut::<AppState>().update_message = None;
        let download_release = release.clone();
        let request = self
            .update_worker(cx)
            .spawn(async move { update::prepare(&download_release, &exe, scope, &space, &config) });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |this, window, cx| {
                cx.global_mut::<AppState>().update_task = None;
                let handoff = match result {
                    Ok(handoff) => handoff,
                    Err(error) => {
                        cx.global_mut::<AppState>().update_state =
                            State::Available(release.clone());
                        cx.global_mut::<AppState>().update_message = Some(error.to_string());
                        cx.notify();
                        return;
                    }
                };
                if this.dir != origin_space {
                    cx.global_mut::<AppState>().update_state = State::Available(release.clone());
                    cx.global_mut::<AppState>().update_message =
                        Some("Space가 변경되었습니다. 현재 Space에서 다시 업데이트하세요.".into());
                    cx.notify();
                    return;
                }
                if !this.prepare_update_close(window, cx) {
                    cx.global_mut::<AppState>().update_state = State::Available(release.clone());
                    cx.global_mut::<AppState>().update_message =
                        Some("저장하지 못해 업데이트를 중단했습니다.".into());
                    cx.notify();
                    return;
                }
                if let Err(error) = handoff.commit() {
                    cx.global_mut::<AppState>().update_state = State::Available(release.clone());
                    cx.global_mut::<AppState>().update_message = Some(error.to_string());
                    cx.notify();
                    return;
                }
                // Data is already saved. The helper waits for this process to exit before Setup.
                window.remove_window();
            });
        });
        cx.global_mut::<AppState>().update_task = Some(task);
        cx.notify();
    }

    pub(super) fn prepare_update_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.startup_failed || !self.finish_rename(true, false, window, cx) || !self.save_all(cx)
        {
            return false;
        }
        if let Err(error) = self.save_session() {
            self.fail("세션을 저장하지 못해 업데이트를 중단했습니다", error, cx);
            return false;
        }
        true
    }
}
