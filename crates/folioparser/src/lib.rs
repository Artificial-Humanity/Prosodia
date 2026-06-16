uniffi::setup_scaffolding!();

mod zip_reader;
use zip_reader::{ZipArchive, ZipReadError};

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::{HashMap, HashSet};
use html_escape;
use regex::Regex;

#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct TocEntry {
    pub title: String,
    pub href: String,
    pub children: Vec<TocEntry>,
}

#[derive(uniffi::Record, Default)]
pub struct ContainerXmlParser {
    pub opf_path: Option<String>,
}

#[uniffi::export]
pub fn parse_container_xml(xml: String) -> ContainerXmlParser {
    let mut parser = ContainerXmlParser::default();
        let mut reader = Reader::from_str(&xml);
        reader.trim_text(true);

        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let local_name = local_name.split(':').last().unwrap_or(local_name);
                    if local_name == "rootfile" {
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                if attr.key.into_inner() == b"full-path" {
                                    parser.opf_path = Some(String::from_utf8_lossy(&attr.value).into_owned());
                                }
                            }
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        parser
    }

#[derive(uniffi::Record, Default)]
pub struct OpfXmlParser {
    pub hrefs: HashMap<String, String>,
    pub spine_item_refs: Vec<String>,
    pub media_types: HashMap<String, String>,
    
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub date: Option<String>,
    pub identifier: Option<String>,
    pub book_description: Option<String>,
    pub subjects: Vec<String>,
    
    pub spine_toc_id: Option<String>,
    pub nav_href: Option<String>,
    
    pub ncx_href: Option<String>,
    pub resolved_cover_href: Option<String>,
}

#[uniffi::export]
pub fn parse_opf_xml(xml: String) -> OpfXmlParser {
    let mut parser = OpfXmlParser::default();
        let mut reader = Reader::from_str(&xml);
        reader.trim_text(true);

        let mut current_element = None;
        let mut temp_characters = String::new();
        
        let mut cover_meta_id = None;
        let mut epub3_cover_href = None;

        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let local_name = local_name.split(':').last().unwrap_or(local_name).to_string();
                    current_element = Some(local_name.clone());
                    temp_characters.clear();
                    
                    if local_name == "item" {
                        let mut id = None;
                        let mut href = None;
                        let mut media_type = None;
                        let mut properties = None;
                        
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                let key = std::str::from_utf8(attr.key.into_inner()).unwrap_or("");
                                let val = String::from_utf8_lossy(&attr.value).into_owned();
                                match key {
                                    "id" => id = Some(val),
                                    "href" => href = Some(val),
                                    "media-type" => media_type = Some(val),
                                    "properties" => properties = Some(val),
                                    _ => {}
                                }
                            }
                        }
                        
                        if let (Some(id), Some(href)) = (id, href.clone()) {
                            parser.hrefs.insert(id.clone(), href.clone());
                            if let Some(mt) = media_type {
                                parser.media_types.insert(id.clone(), mt);
                            }
                            if let Some(props) = properties {
                                if props.contains("cover-image") {
                                    epub3_cover_href = Some(href.clone());
                                }
                                if props.contains("nav") {
                                    parser.nav_href = Some(href);
                                }
                            }
                        }
                    } else if local_name == "itemref" {
                        let mut idref = None;
                        let mut linear = None;
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                let key = std::str::from_utf8(attr.key.into_inner()).unwrap_or("");
                                let val = String::from_utf8_lossy(&attr.value).into_owned();
                                if key == "idref" { idref = Some(val.clone()); }
                                if key == "linear" { linear = Some(val); }
                            }
                        }
                        if let Some(id) = idref {
                            if linear.as_deref() != Some("no") {
                                parser.spine_item_refs.push(id);
                            }
                        }
                    } else if local_name == "spine" {
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                if attr.key.into_inner() == b"toc" {
                                    parser.spine_toc_id = Some(String::from_utf8_lossy(&attr.value).into_owned());
                                }
                            }
                        }
                    } else if local_name == "meta" {
                        let mut name = None;
                        let mut content = None;
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                let key = std::str::from_utf8(attr.key.into_inner()).unwrap_or("");
                                let val = String::from_utf8_lossy(&attr.value).into_owned();
                                if key == "name" { name = Some(val.clone()); }
                                if key == "content" { content = Some(val); }
                            }
                        }
                        if name.as_deref() == Some("cover") {
                            cover_meta_id = content;
                        }
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if let Some(el) = &current_element {
                        if ["title", "creator", "language", "publisher", "date", "identifier", "description", "subject"].contains(&el.as_str()) {
                            temp_characters.push_str(&e.unescape().unwrap_or_default());
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let local_name = local_name.split(':').last().unwrap_or(local_name).to_string();
                    let val = temp_characters.trim().to_string();
                    
                    if !val.is_empty() {
                        match local_name.as_str() {
                            "title" => parser.title = Some(val),
                            "creator" => parser.author = Some(val),
                            "language" => parser.language = Some(val),
                            "publisher" => parser.publisher = Some(val),
                            "date" => parser.date = Some(val),
                            "identifier" => parser.identifier = Some(val),
                            "description" => parser.book_description = Some(val),
                            "subject" => parser.subjects.push(val),
                            _ => {}
                        }
                    }
                    
                    if current_element.as_ref() == Some(&local_name) {
                        current_element = None;
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        
        if let Some(ref toc_id) = parser.spine_toc_id {
            parser.ncx_href = parser.hrefs.get(toc_id).cloned();
        }
        
        parser.resolved_cover_href = epub3_cover_href.or_else(|| {
            cover_meta_id.and_then(|id| parser.hrefs.get(&id).cloned())
        });
        
        parser
    }

#[derive(uniffi::Record, Default)]
pub struct Epub3NavParser {
    pub toc_entries: Vec<TocEntry>,
}

#[uniffi::export]
pub fn parse_epub3_nav(xml: String) -> Epub3NavParser {
    let mut parser = Epub3NavParser::default();
        let mut reader = Reader::from_str(&xml);
        reader.trim_text(true);
        reader.expand_empty_elements(true);
        
        let mut in_toc_nav = false;
        let mut nav_depth = 0;
        let mut current_href = None;
        let mut temp_characters = String::new();
        
        let mut entry_stack: Vec<TocEntry> = Vec::new();
        let mut has_last_created_entry = false;
        
        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let local_name = local_name.split(':').last().unwrap_or(local_name).to_string();
                    
                    if in_toc_nav {
                        nav_depth += 1;
                        if local_name == "a" {
                            for attr in e.attributes() {
                                if let Ok(attr) = attr {
                                    if attr.key.into_inner() == b"href" {
                                        current_href = Some(String::from_utf8_lossy(&attr.value).into_owned());
                                    }
                                }
                            }
                            temp_characters.clear();
                        } else if local_name == "ol" || local_name == "ul" {
                            if has_last_created_entry {
                                let last_val = if let Some(parent) = entry_stack.last_mut() {
                                    parent.children.pop().unwrap()
                                } else {
                                    parser.toc_entries.pop().unwrap()
                                };
                                entry_stack.push(last_val);
                                has_last_created_entry = false;
                            }
                        }
                    } else if local_name == "nav" {
                        let mut nav_type = None;
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                let key = std::str::from_utf8(attr.key.into_inner()).unwrap_or("");
                                if key == "epub:type" || key == "type" {
                                    nav_type = Some(String::from_utf8_lossy(&attr.value).into_owned());
                                }
                            }
                        }
                        if nav_type.as_deref() == Some("toc") {
                            in_toc_nav = true;
                            nav_depth = 1;
                            entry_stack.clear();
                            has_last_created_entry = false;
                        }
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_toc_nav && current_href.is_some() {
                        temp_characters.push_str(&e.unescape().unwrap_or_default());
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let local_name = local_name.split(':').last().unwrap_or(local_name).to_string();
                    
                    if in_toc_nav {
                        if local_name == "a" {
                            if let Some(href) = current_href.take() {
                                let title = temp_characters.trim().to_string();
                                if !title.is_empty() {
                                    let entry = TocEntry {
                                        title,
                                        href,
                                        children: Vec::new(),
                                    };
                                    if let Some(parent) = entry_stack.last_mut() {
                                        parent.children.push(entry.clone());
                                    } else {
                                        parser.toc_entries.push(entry.clone());
                                    }
                                    has_last_created_entry = true;
                                }
                            }
                        } else if local_name == "ol" || local_name == "ul" {
                            if !entry_stack.is_empty() {
                                let completed = entry_stack.pop().unwrap();
                                if let Some(parent) = entry_stack.last_mut() {
                                    parent.children.push(completed);
                                } else {
                                    parser.toc_entries.push(completed);
                                }
                            }
                        }
                        
                        nav_depth -= 1;
                        if nav_depth == 0 {
                            in_toc_nav = false;
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        
        parser
    }

#[derive(uniffi::Record, Default)]
pub struct Epub2NcxParser {
    pub toc_entries: Vec<TocEntry>,
}

struct NavPointBuilder {
    id: String,
    title: String,
    href: String,
    children: Vec<TocEntry>,
}

#[uniffi::export]
pub fn parse_epub2_ncx(xml: String) -> Epub2NcxParser {
    let mut parser = Epub2NcxParser::default();
        let mut reader = Reader::from_str(&xml);
        reader.trim_text(true);
        reader.expand_empty_elements(true);
        
        let mut builder_stack: Vec<NavPointBuilder> = Vec::new();
        let mut in_text = false;
        
        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let local_name = local_name.split(':').last().unwrap_or(local_name);
                    
                    if local_name == "navPoint" {
                        let mut id = String::new();
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                if attr.key.into_inner() == b"id" {
                                    id = String::from_utf8_lossy(&attr.value).into_owned();
                                }
                            }
                        }
                        builder_stack.push(NavPointBuilder {
                            id,
                            title: String::new(),
                            href: String::new(),
                            children: Vec::new(),
                        });
                    } else if local_name == "text" && !builder_stack.is_empty() {
                        in_text = true;
                    } else if local_name == "content" && !builder_stack.is_empty() {
                        let mut src = String::new();
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                if attr.key.into_inner() == b"src" {
                                    src = String::from_utf8_lossy(&attr.value).into_owned();
                                }
                            }
                        }
                        if let Some(last) = builder_stack.last_mut() {
                            last.href = src;
                        }
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_text && !builder_stack.is_empty() {
                        if let Some(last) = builder_stack.last_mut() {
                            last.title.push_str(&e.unescape().unwrap_or_default());
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let local_name = local_name.split(':').last().unwrap_or(local_name);
                    
                    if local_name == "text" {
                        in_text = false;
                    } else if local_name == "navPoint" {
                        if let Some(completed_builder) = builder_stack.pop() {
                            let title = completed_builder.title.trim().to_string();
                            let entry = TocEntry {
                                title,
                                href: completed_builder.href,
                                children: completed_builder.children,
                            };
                            
                            if let Some(parent) = builder_stack.last_mut() {
                                parent.children.push(entry);
                            } else {
                                parser.toc_entries.push(entry);
                            }
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        parser
    }

#[derive(uniffi::Record, Clone)]
pub struct EpubTextExtractionOptions {
    pub ignored_tags: Vec<String>,
    pub ignored_classes: Vec<String>,
}

#[uniffi::export]
pub fn default_epub_text_extraction_options() -> EpubTextExtractionOptions {
    EpubTextExtractionOptions {
            ignored_tags: vec!["head".into(), "style".into(), "script".into(), "aside".into(), "table".into(), "footer".into(), "nav".into()],
            ignored_classes: vec!["footnote".into(), "aside".into(), "nav".into(), "toc".into(), "ad".into(), "advertisement".into()],
        }
    }

#[derive(uniffi::Object)]
pub struct EpubTextExtractor;

#[uniffi::export]
impl EpubTextExtractor {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self)
    }

    pub fn extract_plain_text(&self, xhtml: String, options: EpubTextExtractionOptions) -> String {
        let mut preprocessed = xhtml.clone();
        let entities = [
            ("&nbsp;", " "),
            ("&ldquo;", "“"),
            ("&rdquo;", "”"),
            ("&lsquo;", "‘"),
            ("&rsquo;", "’"),
            ("&laquo;", "«"),
            ("&raquo;", "»"),
            ("&mdash;", "—"),
            ("&ndash;", "–"),
            ("&hellip;", "…")
        ];
        for (entity, unicode) in entities {
            preprocessed = preprocessed.replace(entity, unicode);
        }
        
        let mut reader = Reader::from_str(&preprocessed);
        reader.trim_text(false);
        reader.expand_empty_elements(true);
        
        let ignored_tags: HashSet<String> = options.ignored_tags.clone().into_iter().collect();
        let ignored_classes: HashSet<String> = options.ignored_classes.clone().into_iter().collect();
        
        let mut extracted_text = String::new();
        let mut ignored_tags_stack: Vec<String> = Vec::new();
        
        let block_elements: HashSet<&str> = ["p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "tr", "br", "aside", "table", "section", "article", "header", "footer", "nav"].into_iter().collect();
        
        let mut success = true;

        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let lower_name = local_name.split(':').last().unwrap_or(local_name).to_lowercase();
                    
                    if block_elements.contains(lower_name.as_str()) {
                        if !extracted_text.is_empty() && !extracted_text.ends_with('\n') {
                            extracted_text.push('\n');
                        }
                    }
                    
                    let mut should_ignore = ignored_tags.contains(&lower_name);
                    if !should_ignore && !ignored_classes.is_empty() {
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                if attr.key.into_inner() == b"class" {
                                    let classes_str = String::from_utf8_lossy(&attr.value);
                                    for cls in classes_str.split_whitespace() {
                                        if ignored_classes.contains(cls) {
                                            should_ignore = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    if should_ignore {
                        ignored_tags_stack.push(lower_name);
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if ignored_tags_stack.is_empty() {
                        let text = e.unescape().unwrap_or_default().into_owned();
                        extracted_text.push_str(&text);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local_name = std::str::from_utf8(e.name().into_inner()).unwrap_or("");
                    let lower_name = local_name.split(':').last().unwrap_or(local_name).to_lowercase();
                    
                    if let Some(last) = ignored_tags_stack.last() {
                        if last == &lower_name {
                            ignored_tags_stack.pop();
                        }
                    }
                    
                    if block_elements.contains(lower_name.as_str()) {
                        if !extracted_text.is_empty() && !extracted_text.ends_with('\n') {
                            extracted_text.push('\n');
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => {
                    success = false;
                    break;
                }
                _ => {}
            }
        }
        
        if success {
            let processed = extracted_text
                .lines()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            return processed;
        }
        
        self.plain_text_fallback(&xhtml, &options, &ignored_classes)
    }
}

impl EpubTextExtractor {
    fn plain_text_fallback(
        &self,
        xhtml: &str,
        options: &EpubTextExtractionOptions,
        ignored_classes: &HashSet<String>,
    ) -> String {
        let mut result = String::new();
        let mut in_tag = false;
        let mut tag_buffer = String::new();
        let mut active_ignored_tags_stack: Vec<String> = Vec::new();
        let ignored_tags: HashSet<String> = options.ignored_tags.iter().cloned().collect();
        
        let chars: Vec<char> = xhtml.chars().collect();
        let mut i = 0;
        
        while i < chars.len() {
            let c = chars[i];
            if c == '<' {
                in_tag = true;
                tag_buffer.clear();
            } else if c == '>' {
                in_tag = false;
                let trimmed_tag = tag_buffer.trim().to_lowercase();
                
                if trimmed_tag.starts_with('/') {
                    let end_tag_name = trimmed_tag[1..].trim().to_string();
                    if let Some(last) = active_ignored_tags_stack.last() {
                        if last == &end_tag_name {
                            active_ignored_tags_stack.pop();
                        }
                    }
                } else if !trimmed_tag.ends_with('/') {
                    let (tag_name, should_ignore) = self.should_ignore_tag(&trimmed_tag, &ignored_tags, ignored_classes);
                    if should_ignore {
                        active_ignored_tags_stack.push(tag_name);
                    }
                }
            } else {
                if in_tag {
                    tag_buffer.push(c);
                } else {
                    if active_ignored_tags_stack.is_empty() {
                        result.push(c);
                    }
                }
            }
            i += 1;
        }
        
        let decoded = self.decode_html_entities(&result);
        
        decoded.lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn should_ignore_tag(
        &self,
        tag_content: &str,
        ignored_tags: &HashSet<String>,
        ignored_classes: &HashSet<String>,
    ) -> (String, bool) {
        let tokens: Vec<&str> = tag_content.split_whitespace().collect();
        if tokens.is_empty() {
            return (String::new(), false);
        }
        let tag_name = tokens[0].to_string();
        
        if ignored_tags.contains(&tag_name) {
            return (tag_name, true);
        }
        
        if !ignored_classes.is_empty() {
            if let Some(class_idx) = tag_content.find("class=") {
                let after_class = &tag_content[class_idx + 6..];
                if let Some(quote_char) = after_class.chars().next() {
                    if quote_char == '"' || quote_char == '\'' {
                        let rest = &after_class[1..];
                        if let Some(end_quote_idx) = rest.find(quote_char) {
                            let class_value = &rest[..end_quote_idx];
                            for cls in class_value.split_whitespace() {
                                if ignored_classes.contains(cls) {
                                    return (tag_name, true);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        (tag_name, false)
    }

    fn decode_html_entities(&self, input: &str) -> String {
        let mut result = input.to_string();
        let entities = [
            ("&amp;", "&"),
            ("&lt;", "<"),
            ("&gt;", ">"),
            ("&quot;", "\""),
            ("&apos;", "'"),
            ("&nbsp;", " "),
            ("&ldquo;", "“"),
            ("&rdquo;", "”"),
            ("&lsquo;", "‘"),
            ("&raquo;", "»"),
            ("&rsquo;", "’"),
            ("&laquo;", "«"),
            ("&mdash;", "—"),
            ("&ndash;", "–"),
            ("&hellip;", "…")
        ];
        for (entity, unicode) in entities {
            result = result.replace(entity, unicode);
        }
        
        let re = Regex::new(r"&#(x?)([0-9a-fA-F]+);").unwrap();
        let final_result = re.replace_all(&result, |caps: &regex::Captures| {
            let is_hex = caps.get(1).map_or("", |m| m.as_str()) == "x";
            let radix = if is_hex { 16 } else { 10 };
            if let Some(num_str) = caps.get(2) {
                if let Ok(code) = u32::from_str_radix(num_str.as_str(), radix) {
                    if let Some(scalar) = std::char::from_u32(code) {
                        return scalar.to_string();
                    }
                }
            }
            caps.get(0).unwrap().as_str().to_string()
        });
        
        // Final fallback using html_escape
        html_escape::decode_html_entities(&final_result).into_owned()
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FolioParserError {
    #[error("IO error: {msg}")]
    IoError { msg: String },
    #[error("Zip error: {msg}")]
    ZipError { msg: String },
    #[error("XML error: {msg}")]
    XmlError { msg: String },
    #[error("Missing container.xml")]
    MissingContainer,
    #[error("Missing OPF file: {path}")]
    MissingOpf { path: String },
    #[error("Chapter not found in archive: {path}")]
    MissingChapter { path: String },
}

impl From<std::io::Error> for FolioParserError {
    fn from(err: std::io::Error) -> Self {
        FolioParserError::IoError { msg: err.to_string() }
    }
}

impl From<ZipReadError> for FolioParserError {
    fn from(err: ZipReadError) -> Self {
        match err {
            ZipReadError::Io(e) => FolioParserError::IoError { msg: e.to_string() },
            other => FolioParserError::ZipError { msg: other.to_string() },
        }
    }
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct EpubChapter {
    pub spine_index: i32,
    pub title: Option<String>,
    pub text: String,
}

#[uniffi::export]
pub fn parse_epub(epub_path: String) -> Result<Vec<EpubChapter>, FolioParserError> {
    let archive = ZipArchive::open(&epub_path)?;

    // 1. Read META-INF/container.xml
    let container_bytes = archive.by_name("META-INF/container.xml")?;
    let container_xml = String::from_utf8_lossy(&container_bytes).into_owned();

    // Parse container XML to find OPF path
    let container = parse_container_xml(container_xml);
    let opf_path = container.opf_path.ok_or(FolioParserError::MissingContainer)?;

    // Determine base directory of OPF (everything before the last /)
    let opf_dir = if let Some(idx) = opf_path.rfind('/') {
        &opf_path[..idx]
    } else {
        ""
    };

    // 2. Read OPF file
    let opf_bytes = archive
        .by_name(&opf_path)
        .map_err(|_| FolioParserError::MissingOpf { path: opf_path.clone() })?;
    let opf_xml = String::from_utf8_lossy(&opf_bytes).into_owned();

    // Parse OPF XML to resolve spine and hrefs
    let opf = parse_opf_xml(opf_xml);

    // 3. For each spine itemref, extract the text
    let mut chapters = Vec::new();
    let mut spine_index = 0;
    let extractor = EpubTextExtractor::new();
    let options = default_epub_text_extraction_options();

    for itemref in opf.spine_item_refs {
        if let Some(href) = opf.hrefs.get(&itemref) {
            let relative_path = resolve_zip_path(opf_dir, href);

            match archive.by_name(&relative_path) {
                Ok(bytes) => {
                    let xhtml = String::from_utf8_lossy(&bytes).into_owned();
                    let text = extractor.extract_plain_text(xhtml, options.clone());
                    chapters.push(EpubChapter {
                        spine_index,
                        title: None,
                        text,
                    });
                    spine_index += 1;
                }
                Err(_) => {
                    return Err(FolioParserError::MissingChapter { path: relative_path });
                }
            }
        }
    }

    Ok(chapters)
}

fn resolve_zip_path(base_dir: &str, relative_path: &str) -> String {
    let mut parts: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    
    for part in relative_path.split('/') {
        if part == "." || part.is_empty() {
            continue;
        }
        if part == ".." {
            parts.pop();
        } else {
            parts.push(part);
        }
    }
    
    parts.join("/")
}

