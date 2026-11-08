//! Main window: destination, live state of the link, and the radio's channels.

use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, FontWeight, SharedString,
    Subscription, Window, div, prelude::*, px, relative, rgb,
};
use tx16s_protocol::{AXES, AXIS_MAX};

use crate::bridge::{Bridge, Tone};
use crate::text_input::{Submitted, TextInput};
use crate::theme::{self, ACCENT, BORDER, ERROR, PANEL};

const RADIO_BUTTONS: usize = 24;

pub struct BridgeWindow {
    bridge: Entity<Bridge>,
    target_input: Entity<TextInput>,
    _subscriptions: [Subscription; 2],
}

impl BridgeWindow {
    pub fn new(bridge: Entity<Bridge>, cx: &mut Context<Self>) -> Self {
        let target = bridge.read(cx).config.target.clone();
        let target_input =
            cx.new(|cx| TextInput::new(&target, "Adresse du PC, ex. 10.0.0.200", cx));
        let submit = cx.subscribe(&target_input, |this, _, _: &Submitted, cx| {
            this.apply_target(cx);
        });
        let redraw = cx.observe(&bridge, |_, _, cx| cx.notify());
        Self { bridge, target_input, _subscriptions: [submit, redraw] }
    }

    fn apply_target(&mut self, cx: &mut Context<Self>) {
        let target = self.target_input.read(cx).text().to_owned();
        self.bridge.update(cx, |bridge, cx| bridge.set_target(&target, cx));
    }

    fn toggle_sending(&mut self, cx: &mut Context<Self>) {
        self.apply_target(cx);
        self.bridge.update(cx, |bridge, cx| bridge.toggle_sending(cx));
    }

    fn destination(&self, sending: bool, test_signal: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, color) = if sending { ("Arrêter", ERROR) } else { ("Démarrer", ACCENT) };
        section("PC de destination")
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(self.target_input.clone()))
                    .child(
                        button("toggle-sending", label, color)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_sending(cx))),
                    ),
            )
            .child(
                div()
                    .id("test-signal")
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .text_color(theme::muted())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.bridge.update(cx, |bridge, cx| bridge.toggle_test_signal(cx));
                    }))
                    .child(
                        div()
                            .size(px(14.))
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(if test_signal { ACCENT } else { BORDER }))
                            .bg(rgb(if test_signal { ACCENT } else { theme::FIELD })),
                    )
                    .child("Envoyer un signal de test à la place de la radio"),
            )
    }
}

impl Focusable for BridgeWindow {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.target_input.read(cx).focus_handle(cx)
    }
}

impl Render for BridgeWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bridge = self.bridge.read(cx);
        let input = bridge.snapshot.input;
        let (sending, test_signal) = (bridge.config.sending, bridge.test_signal);
        let status = div()
            .flex()
            .gap_3()
            .child(status_card("Radio", bridge.radio_status()))
            .child(status_card("Envoi", bridge.network_status()))
            .child(status_card("PC", bridge.receiver_status()));
        let destination = self.destination(sending, test_signal, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .pt(px(40.))
            .bg(rgb(theme::BACKGROUND))
            .text_color(theme::text())
            .text_sm()
            .child(destination)
            .child(status)
            .child(section("Voies").children(
                (0..AXES).map(|i| axis_row(i, input.map(|input| input.axes[i]))),
            ))
            .child(buttons(input.map_or(0, |input| input.buttons)))
    }
}

fn section(title: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .rounded_lg()
        .bg(rgb(PANEL))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::muted())
                .child(title.to_uppercase()),
        )
}

fn button(id: &'static str, label: &'static str, color: u32) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_4()
        .py(px(7.))
        .rounded_md()
        .bg(rgb(color))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|style| style.opacity(0.85))
        .child(label)
}

fn status_card(title: &'static str, (text, tone): (String, Tone)) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_lg()
        .bg(rgb(PANEL))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(theme::muted())
                .child(div().size(px(8.)).rounded_full().bg(rgb(theme::tone(tone))))
                .child(title),
        )
        .child(div().text_sm().child(SharedString::from(text)))
}

fn buttons(pressed: u32) -> impl IntoElement {
    section("Boutons").child(div().flex().flex_wrap().gap_1().children((0..RADIO_BUTTONS).map(
        |i| {
            let on = pressed >> i & 1 == 1;
            div()
                .w(px(28.))
                .h(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .text_xs()
                .bg(rgb(if on { ACCENT } else { theme::FIELD }))
                .text_color(if on { theme::text() } else { theme::muted() })
                .child(format!("{}", i + 1))
        },
    )))
}

fn axis_row(index: usize, value: Option<i16>) -> AnyElement {
    let fraction = value.map_or(0.0, |v| f32::from(v) / f32::from(AXIS_MAX));
    let (start, width) = if fraction < 0.0 {
        (0.5 + fraction / 2.0, -fraction / 2.0)
    } else {
        (0.5, fraction / 2.0)
    };
    let label = value.map_or("—".to_owned(), |_| format!("{:+.0} %", fraction * 100.0));
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(div().w(px(36.)).text_color(theme::muted()).child(format!("CH{}", index + 1)))
        .child(
            div()
                .relative()
                .flex_1()
                .h(px(10.))
                .rounded_full()
                .bg(rgb(theme::FIELD))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(start))
                        .w(relative(width))
                        .rounded_full()
                        .bg(rgb(ACCENT)),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(0.5))
                        .w(px(1.))
                        .bg(rgb(BORDER)),
                ),
        )
        .child(div().w(px(56.)).flex().justify_end().child(label))
        .into_any_element()
}
