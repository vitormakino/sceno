//! System-tray menu for the tuner: toggle the overlay and pick the meter style.

use futures::channel::mpsc::UnboundedSender;

use crate::instrument::Instrument;
use crate::meter::MeterStyle;
use crate::{Message, REFERENCES};

pub struct TunerTray {
    pub tx: UnboundedSender<Message>,
    pub enabled: bool,
    pub style: MeterStyle,
    pub a4_hz: f64,
    pub instrument: Instrument,
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
        let ref_idx = REFERENCES
            .iter()
            .position(|&r| (r - self.a4_hz).abs() < 0.5)
            .unwrap_or(1);
        let inst_idx = self.instrument.index();
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
                            let _ = this.tx.unbounded_send(Message::SetMeterStyle(this.style));
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
            SubMenu {
                label: "Referência".into(),
                submenu: vec![
                    RadioGroup {
                        selected: ref_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let hz = REFERENCES.get(idx).copied().unwrap_or(440.0);
                            this.a4_hz = hz;
                            let _ = this.tx.unbounded_send(Message::SetReference(hz));
                        }),
                        options: REFERENCES
                            .iter()
                            .map(|r| RadioItem {
                                label: format!("{r:.0} Hz"),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Instrumento".into(),
                submenu: vec![
                    RadioGroup {
                        selected: inst_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let inst = Instrument::from_idx(idx);
                            this.instrument = inst;
                            let _ = this.tx.unbounded_send(Message::SetInstrument(inst));
                        }),
                        options: Instrument::ALL
                            .iter()
                            .map(|i| RadioItem {
                                label: i.label().into(),
                                ..Default::default()
                            })
                            .collect(),
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
