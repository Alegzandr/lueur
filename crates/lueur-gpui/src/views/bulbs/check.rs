//! The check of devices nothing vouches for: a paired Hue bridge or Nanoleaf
//! controller the last search heard where nothing proves it is the one
//! paired (a new address, or another certificate). Lueur sends it nothing
//! until the user says it is theirs; see `crates/lueur-shell/src/identity.rs`.

use super::BulbsPage;
use super::controls::{Look, btn};
use crate::i18n::{fill, t};
use crate::theme::{size, theme};
use gpui::{AnyElement, App, Context, ElementId, SharedString, div, prelude::*, px};
use lueur_shell::identity::{self, Unverified};

/// A device as it showed itself: a "not mine" holds for the session until
/// it shows up somewhere else or with another certificate.
fn sighting(d: &Unverified) -> String {
    format!("{}|{}|{}", d.id, d.ip, d.changed)
}

/// What the check says of a device.
fn say(d: &Unverified, cx: &App) -> String {
    let vars = [
        ("name", d.name.as_str()),
        ("ip", d.ip.as_str()),
        ("known", d.known_ip.as_str()),
    ];
    let key = if d.needs_pairing {
        "unverified_pinned"
    } else if d.changed {
        "unverified_changed"
    } else {
        "unverified_moved"
    };
    fill(&t(cx, key), &vars)
}

impl BulbsPage {
    /// Ask the shell which devices wait for the user's word, once at start.
    pub(super) fn load_unverified(&mut self, cx: &mut Context<Self>) {
        self.job(
            cx,
            |s| identity::unverified_devices(s),
            |this, result, cx| {
                if let Ok(list) = result {
                    this.lueur.update(cx, |l, cx| {
                        l.unverified = list;
                        cx.notify();
                    });
                }
            },
        );
    }

    /// The devices to check now, those not answered "not mine".
    fn shown_unverified(&self, cx: &App) -> Vec<Unverified> {
        let l = self.lueur.read(cx);
        l.unverified
            .iter()
            .filter(|d| !l.dismissed_devices.contains(&sighting(d)))
            .cloned()
            .collect()
    }

    /// Read out the devices a search brings, not each time the page is
    /// drawn.
    pub(super) fn note_unverified(&mut self, cx: &mut Context<Self>) {
        let list = self.lueur.read(cx).unverified.clone();
        let seen: Vec<String> = list.iter().map(sighting).collect();
        if seen == self.check_seen {
            return;
        }
        let fresh: Vec<String> = self
            .shown_unverified(cx)
            .iter()
            .filter(|d| !self.check_seen.contains(&sighting(d)))
            .map(|d| say(d, cx))
            .collect();
        self.check_seen = seen;
        self.check_news = fresh.join(" ").into();
        cx.notify();
    }

    /// The user says the device is theirs: it is trusted where it is now.
    fn confirm_device(&mut self, d: Unverified, cx: &mut Context<Self>) {
        self.check_busy = true;
        let id = d.id.clone();
        self.job(
            cx,
            move |s| identity::confirm_device(s, &id),
            move |this, result, cx| {
                this.check_busy = false;
                match result {
                    Ok(()) => {
                        this.lueur.update(cx, |l, cx| {
                            l.unverified.retain(|x| x.id != d.id);
                            let text = fill(&l.t("unverified_confirmed"), &[("name", &d.name)]);
                            l.inform(text, cx);
                        });
                        // Its lamps come back through the search, at the
                        // new address.
                        this.scan(super::SEARCH_MS, false, cx);
                    }
                    Err(e) => this.lueur.update(cx, |l, cx| l.fail(&e, cx)),
                }
            },
        );
    }

    /// Pair it again with its button: the one way back for a bridge that
    /// no longer shows its pinned certificate, whose old pairing goes first.
    fn pair_device_again(&mut self, d: Unverified, cx: &mut Context<Self>) {
        let Some(found) = self
            .discovered
            .iter()
            .find(|x| x.id == d.id && x.ip == d.ip)
            .cloned()
        else {
            self.scan(super::SEARCH_MS, false, cx);
            return;
        };
        self.check_busy = true;
        let (id, forget) = (d.id.clone(), d.needs_pairing);
        self.job(
            cx,
            move |s| {
                if forget {
                    identity::forget_device(s, &id)
                } else {
                    Ok(())
                }
            },
            move |this, result, cx| {
                this.check_busy = false;
                match result {
                    Ok(()) => {
                        this.lueur
                            .update(cx, |l, _| l.unverified.retain(|x| x.id != d.id));
                        this.pair(found, cx);
                    }
                    Err(e) => this.lueur.update(cx, |l, cx| l.fail(&e, cx)),
                }
            },
        );
    }

    /// One banner per device to check, above the page's groups.
    pub(super) fn device_check(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.shown_unverified(cx);
        if shown.is_empty() {
            return None;
        }
        let theme = *theme(cx);
        let busy = self.check_busy;
        let banners = shown.into_iter().map(|d| {
            let key = |what: &str| ElementId::Name(SharedString::from(format!("{what}-{}", d.id)));
            let (dismiss, confirm, again) = (d.clone(), d.clone(), d.clone());
            let lueur = self.lueur.clone();
            div()
                .flex()
                .items_center()
                .gap(px(size::S3))
                .py(px(size::S2))
                .pr(px(size::S2))
                .pl(px(size::S4))
                .rounded(px(size::R_LG))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.separator)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(size::S1))
                        .text_size(px(size::T_MD))
                        .child(div().text_color(theme.text).child(say(&d, cx)))
                        .child(
                            div()
                                .text_size(px(size::T_SM))
                                .text_color(theme.text_3)
                                .child(t(cx, "unverified_tip")),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(size::S2))
                        .child(btn(
                            key("not-mine"),
                            t(cx, "unverified_not_mine"),
                            Look::Default,
                            true,
                            busy,
                            None,
                            &theme,
                            move |_, _, cx| {
                                lueur.update(cx, |l, cx| {
                                    l.dismissed_devices.push(sighting(&dismiss));
                                    cx.notify();
                                })
                            },
                        ))
                        // Never the main action: the user's word is the
                        // weakest proof.
                        .when(!d.needs_pairing, |row| {
                            row.child(btn(
                                key("mine"),
                                t(cx, "unverified_mine"),
                                Look::Default,
                                true,
                                busy,
                                None,
                                &theme,
                                cx.listener(move |this, _, _, cx| {
                                    this.confirm_device(confirm.clone(), cx)
                                }),
                            ))
                        })
                        .child(btn(
                            key("pair-again"),
                            t(cx, "unverified_pair_again"),
                            Look::Primary,
                            true,
                            busy,
                            None,
                            &theme,
                            cx.listener(move |this, _, _, cx| {
                                this.pair_device_again(again.clone(), cx)
                            }),
                        )),
                )
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(size::S2))
                .children(banners.collect::<Vec<_>>())
                .into_any_element(),
        )
    }
}
