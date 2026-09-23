//! Configuration requirements recorded by declared build-configuration gates.
//!
//! A grammar names an ordered list of configurations, and a `#[config(since = ...)]` gate both
//! removes its element from a build that excludes it and records that the element needs that
//! configuration. Generation publishes one static [`RequirementNote`] per gated construct and
//! one [`Requires`] implementation per Node. A scan over a parsed value answers two questions:
//! the lowest configuration whose grammar accepts the value, and the first construct, in source
//! order, whose requirement is above a given configuration.

use crate::parsed::ViewCursor;
use crate::span::Span;

/// One configuration of a grammar's ordered configuration list.
///
/// The value is the position in the list, so the ordinary ordering is the grammar's ordering:
/// a greater value is a later, more demanding configuration. The generated grammar module
/// converts between this and its own named enum.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Configuration(u16);

impl Configuration {
    /// The first configuration of the list, which every build admits.
    pub const BASELINE: Self = Self(0);

    /// Creates one configuration from its position in the grammar's list.
    pub const fn from_ordinal(ordinal: u16) -> Self {
        Self(ordinal)
    }

    /// Returns the position of this configuration in the grammar's list.
    pub const fn ordinal(self) -> u16 {
        self.0
    }
}

/// Static description of one declared configuration requirement.
///
/// Generation emits these beside the Node that owns the gated construct. The three message
/// fields are opaque to Recursa: a consumer supplies whatever its own diagnostics need.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequirementNote {
    /// Lowest configuration whose grammar accepts the construct.
    configuration: Configuration,
    /// Name of that configuration, exactly as the grammar declared it.
    configuration_name: &'static str,
    /// Authored path of the construct, such as `ParenQueryRef.alias`.
    site: &'static str,
    /// Whether the requirement belongs to the absence of the value rather than its presence.
    absent: bool,
    /// Opaque code supplied by the gate.
    code: Option<&'static str>,
    /// Opaque message supplied by the gate.
    message: Option<&'static str>,
    /// Opaque hint supplied by the gate.
    hint: Option<&'static str>,
    /// Citation of the source that justifies the gate.
    citation: Option<&'static str>,
}

impl RequirementNote {
    /// Builds one note. Generated code is the only caller.
    #[doc(hidden)]
    #[expect(
        clippy::too_many_arguments,
        reason = "one generated constructor carries the complete authored gate"
    )]
    pub const fn new(
        configuration: u16,
        configuration_name: &'static str,
        site: &'static str,
        absent: bool,
        code: Option<&'static str>,
        message: Option<&'static str>,
        hint: Option<&'static str>,
        citation: Option<&'static str>,
    ) -> Self {
        Self {
            configuration: Configuration(configuration),
            configuration_name,
            site,
            absent,
            code,
            message,
            hint,
            citation,
        }
    }

    /// The lowest configuration whose grammar accepts the construct.
    pub const fn configuration(&self) -> Configuration {
        self.configuration
    }

    /// The declared name of that configuration.
    pub const fn configuration_name(&self) -> &'static str {
        self.configuration_name
    }

    /// The authored path of the gated construct.
    pub const fn site(&self) -> &'static str {
        self.site
    }

    /// Whether the requirement comes from a missing value rather than a present one.
    pub const fn is_absence(&self) -> bool {
        self.absent
    }

    /// The opaque code the gate carries.
    pub const fn code(&self) -> Option<&'static str> {
        self.code
    }

    /// The opaque message the gate carries.
    pub const fn message(&self) -> Option<&'static str> {
        self.message
    }

    /// The opaque hint the gate carries.
    pub const fn hint(&self) -> Option<&'static str> {
        self.hint
    }

    /// The citation of the source that justifies the gate.
    pub const fn citation(&self) -> Option<&'static str> {
        self.citation
    }
}

/// One requirement found in one parsed value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Requirement {
    /// Static description of the gate that produced this requirement.
    note: &'static RequirementNote,
    /// Extent of the construct, when the parse retained provenance.
    span: Option<Span>,
}

impl Requirement {
    /// The static description of the gate.
    pub const fn note(&self) -> &'static RequirementNote {
        self.note
    }

    /// The configuration that this construct needs.
    pub const fn configuration(&self) -> Configuration {
        self.note.configuration
    }

    /// The extent of the construct, when the parse retained provenance.
    pub const fn span(&self) -> Option<Span> {
        self.span
    }
}

/// The running state of one scan over a parsed value.
///
/// A scan is a single pre-order walk in authored source order. It carries a floor, so a
/// requirement at or below the floor is ignored, and a ceiling test, so a subtree that cannot
/// beat the answer so far is never entered.
pub struct RequirementScan {
    /// Requirements at or below this configuration are ignored.
    floor: Configuration,
    /// Whether the scan stops at the first requirement above the floor.
    first_only: bool,
    /// Best answer so far: the earliest construct at the highest configuration seen.
    best: Option<Requirement>,
}

impl RequirementScan {
    /// Starts a scan for the lowest configuration that accepts the value.
    const fn minimum() -> Self {
        Self {
            floor: Configuration::BASELINE,
            first_only: false,
            best: None,
        }
    }

    /// Starts a scan for the first construct whose requirement is above `floor`.
    const fn above(floor: Configuration) -> Self {
        Self {
            floor,
            first_only: true,
            best: None,
        }
    }

    /// Reports whether the scan already has its complete answer.
    #[doc(hidden)]
    pub const fn finished(&self) -> bool {
        self.first_only && self.best.is_some()
    }

    /// Reports whether a subtree whose highest requirement is `ceiling` can be skipped.
    ///
    /// A subtree is entered only when it can still change the answer. Nothing at or below the
    /// floor counts, and, while looking for the minimum, a tie never replaces an earlier
    /// construct, so a ceiling equal to the best so far is also uninteresting.
    #[doc(hidden)]
    pub fn skips(&self, ceiling: Configuration) -> bool {
        if self.finished() {
            return true;
        }
        let bar = match &self.best {
            Some(best) if !self.first_only => best.configuration().max(self.floor),
            _ => self.floor,
        };
        ceiling <= bar
    }

    /// Records one construct's requirement at its occurrence.
    #[doc(hidden)]
    pub fn record(&mut self, note: &'static RequirementNote, cursor: ViewCursor<'_>) {
        if self.finished() || note.configuration <= self.floor {
            return;
        }
        let beaten = self
            .best
            .is_none_or(|best| note.configuration > best.configuration());
        if beaten {
            self.best = Some(Requirement {
                note,
                span: cursor.span(),
            });
        }
    }

    /// Returns the answer this scan reached.
    const fn answer(self) -> Option<Requirement> {
        self.best
    }
}

/// The configuration a parsed value needs, derived from the grammar's declared gates.
///
/// Generation implements this for every Node of a grammar whose root declaration lists
/// `Requires` in `derives(...)`.
pub trait Requires {
    /// No construct inside a value of this type requires a configuration above this one.
    ///
    /// Generation computes this by a fixpoint over the reachable type graph, so a scan skips a
    /// whole subtree that has no interesting gate. A grammar region with no gate costs one
    /// comparison.
    const REQUIREMENT_CEILING: Configuration;

    /// Visits every requirement of this value in source order. Generated code only.
    #[doc(hidden)]
    fn scan_requirements(&self, cursor: ViewCursor<'_>, scan: &mut RequirementScan);

    /// Returns the lowest configuration whose grammar accepts this value, together with the
    /// first construct, in source order, that sets it.
    ///
    /// `None` means the baseline configuration accepts the whole value. The span is present
    /// only when the value came from a parse that retained provenance; use
    /// [`crate::Parsed::minimum_requirement`], or
    /// [`crate::ArenaParsed::minimum_requirement`] for an arena AST, to keep it.
    fn minimum_requirement(&self) -> Option<Requirement> {
        let mut scan = RequirementScan::minimum();
        self.scan_requirements(ViewCursor::detached(), &mut scan);
        scan.answer()
    }

    /// Returns the first construct, in source order, whose requirement is above `floor`.
    ///
    /// This is the question a consumer asks about a target older than the build: the answer is
    /// the construct that the older configuration rejects first.
    fn requirement_above(&self, floor: Configuration) -> Option<Requirement> {
        let mut scan = RequirementScan::above(floor);
        self.scan_requirements(ViewCursor::detached(), &mut scan);
        scan.answer()
    }
}

/// Runs one scan from a root occurrence cursor, so every answer keeps its span.
pub(crate) fn scan_from_cursor<T: Requires + ?Sized>(
    value: &T,
    cursor: ViewCursor<'_>,
    mut scan: RequirementScan,
) -> Option<Requirement> {
    if !scan.skips(T::REQUIREMENT_CEILING) {
        value.scan_requirements(cursor, &mut scan);
    }
    scan.answer()
}

/// Starts a minimum scan. Used by `Parsed` and `ArenaParsed`.
pub(crate) const fn minimum_scan() -> RequirementScan {
    RequirementScan::minimum()
}

/// Starts a first-above scan. Used by `Parsed` and `ArenaParsed`.
pub(crate) const fn above_scan(floor: Configuration) -> RequirementScan {
    RequirementScan::above(floor)
}
