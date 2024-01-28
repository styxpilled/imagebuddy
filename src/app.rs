use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime},
};

use egui::{load::SizedTexture, ColorImage, ScrollArea, TextureHandle};
use image::EncodableLayout;

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct ImageBuddy {
    // #[serde(skip)] // This how you opt-out of serialization of a field
    label: String,

    pwd: PathBuf,
    file_index: Option<usize>,

    files: Vec<String>,
    list_files: bool,

    playing: bool,
    framerate: f64,

    #[serde(skip)]
    prev_frame_time: Instant,

    #[serde(skip)]
    image_cache: HashMap<String, TextureHandle>,

    #[serde(skip)]
    loader_threads: Vec<Worker>,

    #[serde(skip)]
    image_job_receiver: crossbeam_channel::Receiver<(String, ColorImage)>,
    #[serde(skip)]
    image_job_sender: crossbeam_channel::Sender<String>,
}

struct ThreadPool {
    _workers: Vec<Worker>,
}

impl ThreadPool {
    fn new(
        receiver: crossbeam_channel::Receiver<String>,
        sender: crossbeam_channel::Sender<(String, ColorImage)>,
    ) -> Self {
        let mut _workers = vec![];

        for id in 0..5 {
            _workers.push(Worker::new(id, receiver.clone(), sender.clone()));
        }

        Self { _workers }
    }
}

struct Worker {
    _id: usize,
    _thread: JoinHandle<()>,
}

impl Worker {
    fn new(
        id: usize,
        receiver: crossbeam_channel::Receiver<String>,
        sender: crossbeam_channel::Sender<(String, ColorImage)>,
    ) -> Self {
        let _id = id.clone();
        let _thread = std::thread::spawn(move || loop {
            if let Ok(path) = receiver.recv() {
                println!("Running on thread: {}; Path: {}", id, &path);

                let img = image::open(&path).unwrap();

                let img = egui::ColorImage::from_rgba_premultiplied(
                    [
                        img.width().try_into().unwrap(),
                        img.height().try_into().unwrap(),
                    ],
                    img.to_rgba8().as_bytes(),
                );

                sender.send((path, img)).unwrap();
            };
        });

        Self { _id, _thread }
    }
}

impl Default for ImageBuddy {
    fn default() -> Self {
        let pwd = std::env::current_dir().unwrap_or_default();

        let files = if pwd.exists() && pwd.is_dir() {
            _ = std::env::set_current_dir(&pwd);
            get_files(&pwd)
        } else {
            vec![]
        };

        let file_index = if files.len() > 0 { Some(0) } else { None };

        let (app_sender, worker_receiver) = crossbeam_channel::unbounded::<String>();
        let (worker_sender, app_receiver) = crossbeam_channel::unbounded::<(String, ColorImage)>();

        ThreadPool::new(worker_receiver, worker_sender);

        Self {
            label: pwd.to_str().unwrap().to_owned(),
            pwd,
            file_index,
            list_files: false,
            files,
            playing: false,
            framerate: 25.0,
            prev_frame_time: Instant::now(),
            image_cache: HashMap::new(),
            loader_threads: vec![],
            image_job_sender: app_sender,
            image_job_receiver: app_receiver,
        }
    }
}

// TODO:
// - deleting files
// - zooming in / out
// - - https://github.com/emilk/egui/issues/1811
// - - https://github.com/emilk/egui/pull/3906
// - copy to clipboard
// - - context menu
// - quick browse with scroll
// - better navigation
// - - navbar
// - - show directories
// - filling cache for previous files
// - opening a file directly
// - - drag & drop // file association
// - - using file menu dialog
// - opening a url
// - - download button // dialog
// - opening a file without opening a directory
// - seeking in playback mode
// - logging
// - settings
// - - control font size
// - better image recognition
// - filter broken images
// - watch pwd with notify
// FIXME:
// - blocking image load
// - - Move worker load into a function
// - forcing image load on click
impl ImageBuddy {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        let mut app: ImageBuddy = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        };

        if let Some(file_index) = app.file_index {
            for n in file_index..app.files.len() - 1 {
                app.request_preload_image((n - file_index).try_into().unwrap());
            }
        };

        app
    }

    fn incr_file_index(&mut self) {
        if self.files.len() > 0 {
            let mut file_index = self.file_index.unwrap_or_default();
            file_index = if file_index == self.files.len() - 1 {
                0
            } else {
                file_index + 1
            };

            self.file_index = Some(file_index);
            self.request_preload_image(1);
        }
    }

    fn decr_file_index(&mut self) {
        if self.files.len() > 0 {
            self.file_index = Some(if self.file_index.unwrap_or_default() == 0 {
                self.files.len() - 1
            } else {
                self.file_index.unwrap_or_default() - 1
            })
        }
    }

    fn wrap_file_index(&self, index: usize) -> usize {
        if index >= self.files.len() {
            index - self.files.len()
        } else {
            index
        }
    }

    // TODO: last image not loading
    fn request_preload_image(&mut self, offset: i32) {
        if let Some(file_index) = self.file_index {
            let path = format!(
                "{}/{}",
                self.label,
                self.files[self.wrap_file_index(((file_index as i32) + offset) as usize)],
            );

            if self.image_cache.get(&path).is_some() {
                return;
            }

            _ = self.image_job_sender.send(path);
        }
    }

    fn preload_image(&mut self, ctx: &egui::Context, path: String, img: ColorImage) {
        let handle = ctx.load_texture("name", img, egui::TextureOptions::default());
        self.image_cache.insert(path, handle);
    }
}

impl eframe::App for ImageBuddy {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(message) = self.image_job_receiver.try_recv() {
            self.preload_image(ctx, message.0, message.1);
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(16.0);

                egui::widgets::global_dark_light_mode_buttons(ui);
                egui::warn_if_debug_build(ui);
            });
        });

        // File list
        if self.list_files {
            egui::SidePanel::new(egui::panel::Side::Left, egui::Id::new("file_list_panel")).show(
                ctx,
                |ui| {
                    ScrollArea::vertical().auto_shrink(true).show(ui, |ui| {
                        for (index, file) in self.files.clone().into_iter().enumerate() {
                            if ui.button(file).clicked() {
                                self.file_index = Some(index);
                            }
                        }
                    });
                },
            );
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::ArrowRight) {
                    self.incr_file_index();
                } else if i.key_pressed(egui::Key::ArrowLeft) {
                    self.decr_file_index();
                }
                if i.key_pressed(egui::Key::Space) {
                    self.playing = !self.playing;
                }
            });

            if self.playing {
                let now = Instant::now();
                if now.duration_since(self.prev_frame_time)
                    > Duration::from_secs_f64(1.0 / self.framerate)
                {
                    self.prev_frame_time = now;
                    self.incr_file_index();
                }

                ctx.request_repaint_after(Duration::from_secs_f64(1.0 / self.framerate));
                // TODO
                // ctx.forget_image(uri)
            }

            // Topbar
            ui.horizontal(|ui| {
                ui.label("PWD: ");
                if ui.text_edit_singleline(&mut self.label).changed() {
                    self.label = self.label.replace("\\", "/");
                    let path = Path::new(&self.label);
                    if path.exists() && path.is_dir() && path != self.pwd.as_path() {
                        _ = std::env::set_current_dir(path);
                        self.pwd = PathBuf::from(path);
                        self.files = get_files(path);
                        self.file_index = if self.files.len() > 0 { Some(0) } else { None };
                        self.image_cache.clear();
                    }
                };
                ui.checkbox(&mut self.list_files, "List files:");
                ui.add(
                    egui::DragValue::new(&mut self.framerate)
                        .speed(0.1)
                        .clamp_range(1..=9999)
                        .prefix("Framerate: "),
                );
                if let Some(file_index) = self.file_index {
                    ui.label(format!("Frame: {}", &file_index));
                    ui.label(format!("File: {}", self.files[file_index]));
                }
            });

            // Image
            if let Some(file_index) = self.file_index {
                ui.vertical_centered_justified(|ui| {
                    if let Some(image) = self
                        .image_cache
                        .get(&format!("{}/{}", self.label, self.files[file_index]))
                    {
                        ui.add(
                            egui::Image::from_texture(SizedTexture::from_handle(image))
                                .fit_to_exact_size(ui.available_size()),
                        );
                    }
                    ui.add(egui::ProgressBar::new(
                        file_index as f32 / self.files.len() as f32,
                    ));
                });
            }
        });
    }
}

fn get_files<P: AsRef<Path>>(path: P) -> Vec<String> {
    let mut files: Vec<(String, SystemTime)> = std::fs::read_dir(path)
        .unwrap()
        .filter(|direntry| direntry.is_ok())
        .map(|e| e.unwrap())
        .filter(|e| e.metadata().unwrap().is_file())
        .map(|e| (e.path(), e.metadata().unwrap().modified().unwrap()))
        .filter(|e| is_image(&e.0))
        .map(|e| (e.0.file_name().unwrap().to_str().unwrap().to_owned(), e.1))
        .collect();

    // files.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    files.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    files.into_iter().map(|file| file.0).collect()
}

fn is_image(path: &PathBuf) -> bool {
    match path.extension().unwrap_or_default().to_str().unwrap() {
        "jpg" | "png" => true,
        _ => false,
    }
}
