use super::*;

#[derive(Clone, Copy)]
pub enum IconName {
    Check,
    ChevronsLeft,
    ChevronsRight,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    FolderOpen,
    Pencil,
    Trash,
    Folder,
    FileText,
    Close,
    Plus,
    Settings,
    RefreshCw,
    ChevronsUpDown,
    ArrowRight,
}
impl IconName {
    fn path(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::ChevronsLeft => "chevrons-left",
            Self::ChevronsRight => "chevrons-right",
            Self::ChevronDown => "chevron-down",
            Self::ChevronRight => "chevron-right",
            Self::ChevronUp => "chevron-up",
            Self::FolderOpen => "folder-open",
            Self::Pencil => "pencil",
            Self::Trash => "trash",
            Self::Folder => "folder",
            Self::FileText => "file-text",
            Self::Close => "x",
            Self::Plus => "plus",
            Self::Settings => "settings",
            Self::RefreshCw => "refresh-cw",
            Self::ChevronsUpDown => "chevrons-up-down",
            Self::ArrowRight => "arrow-right",
        }
    }
}
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    style: StyleRefinement,
}
impl Icon {
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            style: StyleRefinement::default(),
        }
    }
}
impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let mut el = svg()
            .path(self.name.path())
            .size(px(16.))
            .flex_shrink_0()
            .text_color(window.text_style().color);
        el.style().refine(&self.style);
        el
    }
}
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "check" => include_bytes!("../../assets/icons/check.svg"),
            "chevrons-left" => include_bytes!("../../assets/icons/chevrons-left.svg"),
            "chevrons-right" => include_bytes!("../../assets/icons/chevrons-right.svg"),
            "chevron-down" => include_bytes!("../../assets/icons/chevron-down.svg"),
            "chevron-right" => include_bytes!("../../assets/icons/chevron-right.svg"),
            "chevron-up" => include_bytes!("../../assets/icons/chevron-up.svg"),
            "folder-open" => include_bytes!("../../assets/icons/folder-open.svg"),
            "pencil" => include_bytes!("../../assets/icons/pencil.svg"),
            "trash" => include_bytes!("../../assets/icons/trash.svg"),
            "folder" => include_bytes!("../../assets/icons/folder.svg"),
            "file-text" => include_bytes!("../../assets/icons/file-text.svg"),
            "x" => include_bytes!("../../assets/icons/x.svg"),
            "plus" => include_bytes!("../../assets/icons/plus.svg"),
            "settings" => include_bytes!("../../assets/icons/settings.svg"),
            "refresh-cw" => include_bytes!("../../assets/icons/refresh-cw.svg"),
            "chevrons-up-down" => include_bytes!("../../assets/icons/chevrons-up-down.svg"),
            "arrow-right" => include_bytes!("../../assets/icons/arrow-right.svg"),
            _ => return Ok(None),
        };
        Ok(Some(std::borrow::Cow::Borrowed(bytes)))
    }
    fn list(&self, _: &str) -> Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
