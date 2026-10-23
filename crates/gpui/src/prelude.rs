//! The GPUI prelude is a collection of traits and types that are widely used
//! throughout the library. It is recommended to import this prelude into your
//! application to avoid having to import each trait individually.

pub use crate::AppContext as _;
pub use crate::BorrowAppContext;
pub use crate::Context;
pub use crate::Element;
pub use crate::InteractiveElement;
pub use crate::IntoElement;
pub use crate::ParentElement;
pub use crate::Refineable;
pub use crate::Render;
pub use crate::RenderOnce;
pub use crate::StatefulInteractiveElement;
pub use crate::Styled;
pub use crate::StyledImage;
pub use crate::TaskExt as _;
pub use crate::VisualContext;
pub use crate::util::FluentBuilder;
