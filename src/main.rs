mod mcp;
mod store;
use eframe::egui::{self, Color32, RichText, Vec2};
use store::Store;
const GOLD: Color32 = Color32::from_rgb(224, 184, 119);
const MUTED: Color32 = Color32::from_rgb(137, 143, 154);
struct Studio {
    store: Store,
    page: usize,
    new_open: bool,
    name: String,
    premise: String,
    selected: Option<String>,
    episode_title: String,
    script: String,
    episode_id: Option<String>,
    prompt: String,
    message: String,
}
impl Studio {
    fn new(cc: &eframe::CreationContext<'_>, store: Store) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(17, 19, 24);
        style.visuals.window_fill = Color32::from_rgb(24, 27, 33);
        style.visuals.extreme_bg_color = Color32::from_rgb(13, 15, 19);
        style.visuals.selection.bg_fill = Color32::from_rgb(82, 66, 45);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(32, 36, 44);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(51, 49, 44);
        style.spacing.item_spacing = Vec2::new(12., 12.);
        style.spacing.button_padding = Vec2::new(18., 10.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
        cc.egui_ctx.set_style(style);
        Self {
            store,
            page: 0,
            new_open: false,
            name: String::new(),
            premise: String::new(),
            selected: None,
            episode_title: String::new(),
            script: String::new(),
            episode_id: None,
            prompt: String::new(),
            message: String::new(),
        }
    }
    fn title(ui: &mut egui::Ui, kicker: &str, title: &str, sub: &str) {
        ui.label(RichText::new(kicker).size(11.).color(GOLD).strong());
        ui.add_space(6.);
        ui.label(RichText::new(title).size(34.).strong());
        ui.label(RichText::new(sub).size(14.).color(MUTED));
        ui.add_space(24.);
    }
    fn card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::new()
            .fill(Color32::from_rgb(24, 27, 33))
            .stroke(egui::Stroke::new(1_f32, Color32::from_rgb(43, 47, 55)))
            .corner_radius(12)
            .inner_margin(24.)
            .show(ui, content);
    }
    fn result<T>(&mut self, r: anyhow::Result<T>, ok: &str) {
        self.message = match r {
            Ok(_) => ok.into(),
            Err(e) => e.to_string(),
        };
    }
}
impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 15, 19))
                    .inner_margin(12.),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●  LOCAL STUDIO").color(GOLD).size(10.));
                    ui.separator();
                    ui.label(
                        RichText::new(if self.message.is_empty() {
                            "Your stories stay on this Mac"
                        } else {
                            &self.message
                        })
                        .size(11.)
                        .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("v001  /  FOUNDATION").size(10.).color(MUTED));
                    });
                });
            });
        egui::SidePanel::left("navigation")
            .exact_width(218.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 15, 19))
                    .inner_margin(22.),
            )
            .show(ctx, |ui| {
                ui.add_space(16.);
                ui.label(RichText::new("D / F").size(30.).color(GOLD).strong());
                ui.add_space(8.);
                ui.label(RichText::new("DRAMAFACTORY").size(15.).strong());
                ui.label(
                    RichText::new("THE INDEPENDENT STUDIO")
                        .size(9.)
                        .color(MUTED),
                );
                ui.add_space(42.);
                for (i, label) in [
                    "01   Studio",
                    "02   Series & episodes",
                    "03   Production queue",
                    "04   Model library",
                    "05   Preferences",
                ]
                .iter()
                .enumerate()
                {
                    if ui
                        .add_sized(
                            [174., 42.],
                            egui::Button::new(RichText::new(*label).color(if self.page == i {
                                GOLD
                            } else {
                                MUTED
                            }))
                            .selected(self.page == i),
                        )
                        .clicked()
                    {
                        self.page = i;
                    }
                    ui.add_space(3.);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("BUILT FOR YOUR NEXT STORY.")
                            .size(9.)
                            .color(MUTED),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new("Private by design\nLocal-first production")
                            .size(12.)
                            .color(MUTED),
                    );
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(17,19,24)).inner_margin(36.)).show(ctx,|ui|{egui::ScrollArea::vertical().show(ui,|ui|{match self.page{
 0=>{Self::title(ui,"YOUR PRODUCTION HOME","Every great drama starts here.","From the first idea to the final frame. One independent studio.");
 let width=ui.available_width();let (rect,_)=ui.allocate_exact_size(Vec2::new(width,220.),egui::Sense::hover());let p=ui.painter();p.rect_filled(rect,16,Color32::from_rgb(33,34,39));for i in 0..36{let x=rect.right()-i as f32*16.;p.line_segment([egui::pos2(x,rect.top()),egui::pos2(x-150.,rect.bottom())],egui::Stroke::new(1_f32,Color32::from_rgba_unmultiplied(224,184,119,18)));}p.text(rect.left_top()+Vec2::new(28.,30.),egui::Align2::LEFT_TOP,"THE DIRECTOR’S CHAIR",egui::FontId::proportional(11.),GOLD);p.text(rect.left_top()+Vec2::new(28.,64.),egui::Align2::LEFT_TOP,"A world waiting\nto be written.",egui::FontId::proportional(34.),Color32::WHITE);p.text(rect.left_bottom()+Vec2::new(28.,-24.),egui::Align2::LEFT_BOTTOM,"YOUR VISION. YOUR CAST. YOUR FINAL CUT.",egui::FontId::proportional(10.),MUTED);
 ui.add_space(22.);ui.horizontal(|ui|{if ui.button(RichText::new("+  Create a series").color(GOLD)).clicked(){self.new_open=true;}
if ui.button("Open production queue  →").clicked(){self.page=2;}});ui.add_space(24.);
 let series=self.store.series().unwrap_or_default();let jobs=self.store.jobs().unwrap_or_default();ui.columns(3,|cols|{for (col,(n,label)) in cols.iter_mut().zip([(series.len(),"SERIES IN DEVELOPMENT"),(jobs.iter().filter(|j|j.status=="blocked").count(),"REQUESTS AWAITING SETUP"),(0,"VERIFIED VIDEO MODELS")]){Self::card(col,|ui|{ui.label(RichText::new(n.to_string()).size(32.).color(GOLD));ui.label(RichText::new(label).size(10.).color(MUTED));});}});ui.add_space(28.);ui.heading("Your productions");ui.add_space(8.);if series.is_empty(){Self::card(ui,|ui|{ui.label(RichText::new("The stage is yours.").size(21.));ui.label(RichText::new("Create your first series to develop its premise, episodes, and screenplay.").color(MUTED));});}
for s in series{Self::card(ui,|ui|{if ui.button(RichText::new(&s.name).size(20.)).clicked(){self.selected=Some(s.id.clone());self.page=1;}ui.label(&s.premise);});ui.add_space(10.);}}
 1=>{Self::title(ui,"DEVELOPMENT","Series & episodes","Build the world. Find the characters. Write the first scene.");if ui.button("+  New series").clicked(){self.new_open=true;}
for s in self.store.series().unwrap_or_default(){if ui.selectable_label(self.selected.as_ref()==Some(&s.id),RichText::new(&s.name).size(19.)).clicked(){self.selected=Some(s.id.clone());self.episode_id=None;self.script.clear();}
if self.selected.as_ref()==Some(&s.id){ui.label(RichText::new(&s.premise).color(MUTED));ui.add_space(12.);ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.episode_title).hint_text("Episode title"));if ui.button("Add episode").clicked(){let r=self.store.create_episode(&s.id,&self.episode_title);self.result(r,"Episode created");self.episode_title.clear();}});for e in self.store.episodes(&s.id).unwrap_or_default(){if ui.selectable_label(self.episode_id.as_ref()==Some(&e.id),&e.title).clicked(){self.episode_id=Some(e.id);self.script=e.screenplay;}}}ui.separator();}
if let Some(id)=self.episode_id.clone(){ui.add_space(12.);ui.label(RichText::new("SCREENPLAY").color(GOLD).size(11.));ui.add(egui::TextEdit::multiline(&mut self.script).desired_width(f32::INFINITY).desired_rows(16).hint_text("INT. YOUR WORLD — NIGHT\n\nIt begins with a moment…"));if ui.button("Save screenplay").clicked(){let r=self.store.save_screenplay(&id,&self.script);self.result(r,"Screenplay saved");}}}
 2=>{Self::title(ui,"PRODUCTION","The render room","Persistent requests. Clear status. Nothing lost between sessions.");Self::card(ui,|ui|{ui.label(RichText::new("Set the scene").size(21.));ui.add(egui::TextEdit::multiline(&mut self.prompt).desired_width(f32::INFINITY).desired_rows(3).hint_text("Describe the shot, light, camera movement, and atmosphere…"));ui.label(RichText::new("Local video generation requires a verified model worker. Requests are saved as blocked until setup is complete.").color(MUTED));if ui.button("Save render request").clicked(){let r=self.store.queue(&self.prompt);self.result(r,"Render request saved · awaiting worker setup");}});ui.add_space(24.);for j in self.store.jobs().unwrap_or_default(){Self::card(ui,|ui|{ui.horizontal(|ui|{ui.label(RichText::new(j.status.to_uppercase()).size(11.).color(GOLD));ui.label(&j.prompt);});
if !j.error.is_empty(){ui.label(RichText::new(&j.error).color(MUTED));}
if j.status=="blocked"&&ui.small_button("Cancel request").clicked(){let r=self.store.cancel(&j.id);self.result(r,"Request canceled");}});ui.add_space(10.);}}
 3=>{Self::title(ui,"INFERENCE","Model library","A deliberate selection of local creative tools.");Self::card(ui,|ui|{ui.label(RichText::new("Wan 2.2 · TI2V 5B").size(24.));ui.label(RichText::new("CANDIDATE  /  NOT INSTALLED").color(GOLD).size(11.));ui.add_space(12.);ui.label("Text and image to video, through an isolated MLX worker.");ui.label(RichText::new("Upstream MLX support documented. Performance and memory use on this Mac are not yet verified. A large checkpoint download is required before benchmarking.").color(MUTED));});}
 _=>{Self::title(ui,"STUDIO PREFERENCES","Made to stay yours.","Local storage, release channels, and application identity.");Self::card(ui,|ui|{ui.label(RichText::new("Project storage").size(20.));ui.label(self.store.path.display().to_string());ui.add_space(16.);ui.label(RichText::new("Updates · bazstudios / DramaFactory").size(20.));ui.label("Version v001 · MIT license");ui.label(RichText::new("Automatic updates are pending a signed GitHub release feed. Planned behavior: check quietly, download verified updates, install after exit. Developer ID signing and notarization can follow when your Apple developer account is ready.").color(MUTED));ui.add_space(16.);ui.label("Cloud inference is disabled. No API credentials are stored.");});}
 }});});
        if self.new_open {
            let mut open = true;
            egui::Window::new("A new production")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(460.)
                .show(ctx, |ui| {
                    ui.label("SERIES TITLE");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.name)
                            .desired_width(f32::INFINITY)
                            .hint_text("Give your story a name"),
                    );
                    ui.label("THE PREMISE");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.premise)
                            .desired_width(f32::INFINITY)
                            .desired_rows(4)
                            .hint_text("Who wants what, and what stands in their way?"),
                    );
                    if ui
                        .button(RichText::new("Create series  →").color(GOLD))
                        .clicked()
                    {
                        match self.store.create_series(&self.name, &self.premise) {
                            Ok(s) => {
                                self.selected = Some(s.id);
                                self.page = 1;
                                self.new_open = false;
                                self.name.clear();
                                self.premise.clear();
                                self.message = "Series created".into();
                            }
                            Err(e) => self.message = e.to_string(),
                        }
                    }
                });
            self.new_open &= open;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }
}
fn main() -> anyhow::Result<()> {
    let store = Store::open(store::default_path())?;
    if std::env::args().any(|x| x == "--mcp") {
        return mcp::run(store);
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DramaFactory · v001")
            .with_inner_size([1320., 860.])
            .with_min_inner_size([1000., 680.]),
        ..Default::default()
    };
    eframe::run_native(
        "DramaFactory",
        opts,
        Box::new(|cc| Ok(Box::new(Studio::new(cc, store)))),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}
