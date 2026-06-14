//! System-tray menu for the tuner: toggle the overlay and pick the meter style.

use futures::channel::mpsc::UnboundedSender;

use crate::meter::MeterStyle;
use crate::Message;

pub struct TunerTray {
    pub tx: UnboundedSender<Message>,
    pub enabled: bool,
    pub style: MeterStyle,
}

impl ksni::Tray for TunerTray {
    fn icon_name(&self) -> String {
        "audio-input-microphone".into()
    }
    fn title(&self) -> String {
        "sceno · tuner".into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let style = self.style;
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
                label: "Medidor".into(),
                submenu: vec![
                    RadioGroup {
                        selected: style.index(),
                        select: Box::new(|this: &mut Self, idx| {
                            this.style = MeterStyle::from_idx(idx);
                            let _ = this
                                .tx
                                .unbounded_send(Message::SetMeterStyle(this.style));
                        }),
                        options: vec![
                            RadioItem {
                                label: MeterStyle::Needle.label().into(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: MeterStyle::CenterBar.label().into(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: MeterStyle::Strobe.label().into(),
                                ..Default::default()
                            },
                        ],
                    }
                    .into(),
                ],
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
