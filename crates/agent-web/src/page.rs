//! Agent-friendly page model.
//!
//! This is NOT the full DOM. It's a simplified, structured representation
//! of a web page designed for LLM consumption: interactive elements,
//! text regions, navigation landmarks.

use serde::{Deserialize, Serialize};

/// An agent-friendly snapshot of a web page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSnapshot {
    pub url: String,
    pub title: String,
    pub elements: Vec<PageElement>,
}

/// A single interactive or informational element on the page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageElement {
    /// Unique selector or reference for this element.
    pub selector: String,

    /// What kind of element this is.
    pub kind: ElementKind,

    /// Visible text content.
    pub text: Option<String>,

    /// For inputs: current value.
    pub value: Option<String>,

    /// Whether this element is interactable.
    pub interactable: bool,
}

/// Classification of page elements for agent consumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementKind {
    Link,
    Button,
    TextInput,
    Select,
    Checkbox,
    Radio,
    Image,
    Heading,
    Paragraph,
    Navigation,
    Form,
    Other(String),
}
