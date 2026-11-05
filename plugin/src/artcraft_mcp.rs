// ArtCraft Suite — MCA / MCP Command Executor for PdfCraftApp

impl PdfCraftApp {
    pub fn poll_artcraft_agent(&mut self) {
        let reqs = self.artcraft_agent.take_tool_requests();
        for req in reqs {
            let res = match req.tool_name.as_str() {
                "pdf.addNote" | "pdf.addText" | "insert_from_chat" | "type_text" => {
                    if self.active.is_none() {
                        self.create_blank();
                    }
                    let text = req
                        .params
                        .get("text")
                        .or_else(|| req.params.get("markdown"))
                        .or_else(|| req.params.get("contents"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    let page = self.active.and_then(|i| self.views.get(i)).map_or(0, |v| v.current);
                    let clean = text.replace("**", "");
                    let _ = self.apply_edit(pdfcraft_engine::Edit::AddAnnotation(pdfcraft_engine::NewAnnotation {
                        page,
                        shape: pdfcraft_engine::Shape::Note {
                            at: [72.0, 720.0],
                            icon: pdfcraft_engine::NoteIcon::Comment,
                        },
                        style: pdfcraft_engine::Style::default(),
                        contents: clean,
                        author: "AI Agent".into(),
                    }));
                    Ok(serde_json::json!({"status": "ok", "page": page + 1}))
                }
                "pdf.rotatePages" => {
                    if self.active.is_none() {
                        self.create_blank();
                    }
                    let deg = req.params.get("degrees").and_then(serde_json::Value::as_i64).unwrap_or(90);
                    if deg < 0 {
                        self.execute("page.rotate_ccw");
                    } else {
                        self.execute("page.rotate");
                    }
                    Ok(serde_json::json!({"status": "ok", "degrees": deg}))
                }
                "pdf.insertBlankPage" => {
                    if self.active.is_none() {
                        self.create_blank();
                    }
                    self.execute("page.insert_blank");
                    Ok(serde_json::json!({"status": "ok"}))
                }
                "execute" => {
                    let cmd = req.params.get("command").and_then(serde_json::Value::as_str).unwrap_or("");
                    let params = req.params.get("params").cloned().unwrap_or(serde_json::json!({}));
                    if self.active.is_none() && !matches!(cmd, "file.open" | "app.preferences") {
                        self.create_blank();
                    }
                    match cmd {
                        "pdf.addNote" | "pdf.addText" | "type_text" => {
                            let txt = params.get("text").or_else(|| params.get("contents")).and_then(serde_json::Value::as_str).unwrap_or("");
                            let page = self.active.and_then(|i| self.views.get(i)).map_or(0, |v| v.current);
                            let _ = self.apply_edit(pdfcraft_engine::Edit::AddAnnotation(pdfcraft_engine::NewAnnotation {
                                page,
                                shape: pdfcraft_engine::Shape::Note {
                                    at: [72.0, 720.0],
                                    icon: pdfcraft_engine::NoteIcon::Comment,
                                },
                                style: pdfcraft_engine::Style::default(),
                                contents: txt.replace("**", ""),
                                author: "AI Agent".into(),
                            }));
                            Ok(serde_json::json!({"status": "ok", "executed": cmd}))
                        }
                        "file.new" | "create.blank" => {
                            self.create_blank();
                            Ok(serde_json::json!({"status": "ok", "executed": "create.blank"}))
                        }
                        _ => {
                            self.execute(cmd);
                            Ok(serde_json::json!({"executed": cmd}))
                        }
                    }
                }
                "inspect_pdf" | "inspect_document" | "get_text" => {
                    let dirty = self.active.is_some_and(|i| self.has_unsaved_work(i));
                    let title = self.active.and_then(|i| crate::canvas::tab_label(self, i)).unwrap_or_else(|| "PDF-Dokument".into());
                    let page_count = self.active.and_then(|i| self.views.get(i)).map(|v| v.page_count()).unwrap_or(0);
                    let cur_page = self.active.and_then(|i| self.views.get(i)).map(|v| v.current + 1).unwrap_or(0);
                    let path = self.active_ids().and_then(|(_, id)| self.session.get(id)).and_then(|d| d.path.clone());
                    Ok(serde_json::json!({
                        "title": title,
                        "dirty": dirty,
                        "page_count": page_count,
                        "current_page": cur_page,
                        "path": path,
                    }))
                }
                "save_document" => {
                    let target = if let Some(p) = req.params.get("path").and_then(serde_json::Value::as_str) {
                        std::path::PathBuf::from(p)
                    } else if let Some(ref path) = self.active_ids().and_then(|(_, id)| self.session.get(id)).and_then(|d| d.path.clone()) {
                        std::path::PathBuf::from(path)
                    } else {
                        self.artcraft_drive.set_or_init_project(None)
                    };
                    let target_str = target.to_string_lossy().to_string();
                    if let Some((_, id)) = self.active_ids() {
                        self.save_doc_to_then(id, &target_str, crate::editing::SaveTarget::As, Box::new(|_| {}));
                        self.artcraft_drive.mark_saved();
                        Ok(serde_json::json!({"saved": true, "path": target_str}))
                    } else {
                        Err("Kein aktives Dokument geöffnet".to_string())
                    }
                }
                other => {
                    if self.active.is_none() {
                        self.create_blank();
                    }
                    self.execute(other);
                    Ok(serde_json::json!({"executed": other}))
                }
            };
            let _ = req.reply_sender.send(res);
        }

        self.artcraft_agent.process_events();
    }

    /// Automatically saves changes into the active Google Drive project folder if enabled.
    pub fn artcraft_autosave_tick(&mut self) {
        let is_dirty = self.active.is_some_and(|i| self.has_unsaved_work(i));
        if self.artcraft_drive.auto_save_enabled && is_dirty && self.artcraft_drive.should_autosave() {
            let target = if let Some(ref path) = self.active_ids().and_then(|(_, id)| self.session.get(id)).and_then(|d| d.path.clone()) {
                std::path::PathBuf::from(path)
            } else {
                self.artcraft_drive.set_or_init_project(None)
            };
            let target_str = target.to_string_lossy().to_string();
            if let Some((_, id)) = self.active_ids() {
                self.save_doc_to_then(id, &target_str, crate::editing::SaveTarget::As, Box::new(|_| {}));
                self.artcraft_drive.mark_saved();
                self.notify(format!("☁️ Gespeichert in {}", self.artcraft_drive.active_project_folder_name()));
            }
        }
    }
}
