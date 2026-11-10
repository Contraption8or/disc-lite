//! Native OS drag-out — what lets a waveform be dragged straight onto
//! Premiere Pro's timeline (or into Explorer, a DAW, ...). Windows takes
//! over the mouse until the drop, so this call blocks until it's done.

use std::path::Path;

pub fn start_file_drag(frame: &eframe::Frame, path: &Path) -> Result<(), String> {
    let icon = drag::Image::Raw(include_bytes!("../assets/drag-icon.png").to_vec());
    drag::start_drag(
        frame,
        drag::DragItem::Files(vec![path.to_path_buf()]),
        icon,
        |_result, _cursor| {},
        drag::Options::default(),
    )
    .map_err(|e| e.to_string())
}
