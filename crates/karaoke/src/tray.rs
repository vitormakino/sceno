//! System-tray menu for karaoke: toggle the overlay and nudge sync offset.

use futures::channel::mpsc::UnboundedSender;

use crate::Message;

/// One nudge step (ms) for the manual sync offset.
const NUDGE_MS: f64 = 100.0;

pub struct KaraokeTray {
    pub tx: UnboundedSender<Message>,
    pub enabled: bool,
    pub offset_ms: f64,
}

impl KaraokeTray {
    fn nudge(&mut self, delta: f64) {
        self.offset_ms += delta;
        let _ = self.tx.unbounded_send(Message::SetOffset(self.offset_ms));
    }
}

impl ksni::Tray for KaraokeTray {
    fn icon_name(&self) -> String {
        "audio-input-microphone".into()
    }
    fn title(&self) -> String {
        "sceno · karaoke".into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            CheckmarkItem {
                label: "Overlay ativo".into(),
                checked: self.enabled,
                activate: Box::new(|this: &mut Self| {
                    this.enabled = !this.enabled;
                    let _ = this.tx.unbounded_send(Message::SetEnabled(this.enabled));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "Sincronia".into(),
                submenu: vec![
                    StandardItem {
                        label: "Adiantar letra (+100ms)".into(),
                        activate: Box::new(|this: &mut Self| this.nudge(NUDGE_MS)),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Atrasar letra (−100ms)".into(),
                        activate: Box::new(|this: &mut Self| this.nudge(-NUDGE_MS)),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Centralizar (0)".into(),
                        activate: Box::new(|this: &mut Self| {
                            this.offset_ms = 0.0;
                            let _ = this.tx.unbounded_send(Message::SetOffset(0.0));
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Restaurar padrões".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.unbounded_send(Message::ResetDefaults);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Sair".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
