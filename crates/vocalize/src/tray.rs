//! System-tray menu for vocalize.

use futures::channel::mpsc::UnboundedSender;

use crate::Message;

pub struct VocalizeTray {
    pub tx: UnboundedSender<Message>,
    pub enabled: bool,
}

impl ksni::Tray for VocalizeTray {
    fn icon_name(&self) -> String {
        "audio-input-microphone".into()
    }
    fn title(&self) -> String {
        "sceno · vocalize".into()
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
