/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Per-node data used in style calculation.

#![deny(missing_docs)]

use dom::TElement;
use properties::ComputedValues;
use properties::longhands::display::computed_value as display;
use restyle_hints::{RESTYLE_CSS_ANIMATIONS, RESTYLE_DESCENDANTS, RESTYLE_LATER_SIBLINGS, RESTYLE_SELF, RestyleHint};
use rule_tree::StrongRuleNode;
use selector_parser::{EAGER_PSEUDO_COUNT, PseudoElement, RestyleDamage, Snapshot};
#[cfg(feature = "servo")] use std::collections::HashMap;
use std::fmt;
#[cfg(feature = "servo")] use std::hash::BuildHasherDefault;
use std::ops::Deref;
use std::sync::Arc;
use stylist::Stylist;
use thread_state;
use traversal::TraversalFlags;

/// The structure that represents the result of style computation. This is
/// effectively a tuple of rules and computed values, that is, the rule node,
/// and the result of computing that rule node's rules, the `ComputedValues`.
#[derive(Clone)]
pub struct ComputedStyle {
    /// The rule node representing the ordered list of rules matched for this
    /// node.
    pub rules: StrongRuleNode,

    /// The computed values for each property obtained by cascading the
    /// matched rules. This can only be none during a transient interval of
    /// the styling algorithm, and callers can safely unwrap it.
    pub values: Option<Arc<ComputedValues>>,
}

impl ComputedStyle {
    /// Trivially construct a new `ComputedStyle`.
    pub fn new(rules: StrongRuleNode, values: Arc<ComputedValues>) -> Self {
        ComputedStyle {
            rules: rules,
            values: Some(values),
        }
    }

    /// Constructs a partial ComputedStyle, whose ComputedVaues will be filled
    /// in later.
    pub fn new_partial(rules: StrongRuleNode) -> Self {
        ComputedStyle {
            rules: rules,
            values: None,
        }
    }

    /// Returns a reference to the ComputedValues. The values can only be null during
    /// the styling algorithm, so this is safe to call elsewhere.
    pub fn values(&self) -> &Arc<ComputedValues> {
        self.values.as_ref().unwrap()
    }

    /// Mutable version of the above.
    pub fn values_mut(&mut self) -> &mut Arc<ComputedValues> {
        self.values.as_mut().unwrap()
    }
}

// We manually implement Debug for ComputedStyle so that we can avoid the
// verbose stringification of ComputedValues for normal logging.
impl fmt::Debug for ComputedStyle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ComputedStyle {{ rules: {:?}, values: {{..}} }}", self.rules)
    }
}

/// A list of styles for eagerly-cascaded pseudo-elements. Lazily-allocated.
#[derive(Clone, Debug)]
pub struct EagerPseudoStyles(Option<Box<[Option<ComputedStyle>]>>);

impl EagerPseudoStyles {
    /// Returns whether there are any pseudo styles.
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// Returns a reference to the style for a given eager pseudo, if it exists.
    pub fn get(&self, pseudo: &PseudoElement) -> Option<&ComputedStyle> {
        debug_assert!(pseudo.is_eager());
        self.0.as_ref().and_then(|p| p[pseudo.eager_index()].as_ref())
    }

    /// Returns a mutable reference to the style for a given eager pseudo, if it exists.
    pub fn get_mut(&mut self, pseudo: &PseudoElement) -> Option<&mut ComputedStyle> {
        debug_assert!(pseudo.is_eager());
        self.0.as_mut().and_then(|p| p[pseudo.eager_index()].as_mut())
    }

    /// Returns true if the EagerPseudoStyles has a ComputedStyle for |pseudo|.
    pub fn has(&self, pseudo: &PseudoElement) -> bool {
        self.get(pseudo).is_some()
    }

    /// Inserts a pseudo-element. The pseudo-element must not already exist.
    pub fn insert(&mut self, pseudo: &PseudoElement, style: ComputedStyle) {
        debug_assert!(!self.has(pseudo));
        if self.0.is_none() {
            self.0 = Some(vec![None; EAGER_PSEUDO_COUNT].into_boxed_slice());
        }
        self.0.as_mut().unwrap()[pseudo.eager_index()] = Some(style);
    }

    /// Removes a pseudo-element style if it exists, and returns it.
    pub fn take(&mut self, pseudo: &PseudoElement) -> Option<ComputedStyle> {
        let result = match self.0.as_mut() {
            None => return None,
            Some(arr) => arr[pseudo.eager_index()].take(),
        };
        let empty = self.0.as_ref().unwrap().iter().all(|x| x.is_none());
        if empty {
            self.0 = None;
        }
        result
    }

    /// Returns a list of the pseudo-elements.
    pub fn keys(&self) -> Vec<PseudoElement> {
        let mut v = Vec::new();
        if let Some(ref arr) = self.0 {
            for i in 0..EAGER_PSEUDO_COUNT {
                if arr[i].is_some() {
                    v.push(PseudoElement::from_eager_index(i));
                }
            }
        }
        v
    }

    /// Sets the rule node for a given pseudo-element, which must already have an entry.
    ///
    /// Returns true if the rule node changed.
    pub fn set_rules(&mut self, pseudo: &PseudoElement, rules: StrongRuleNode) -> bool {
        debug_assert!(self.has(pseudo));
        let mut style = self.get_mut(pseudo).unwrap();
        let changed = style.rules != rules;
        style.rules = rules;
        changed
    }
}

/// A cache of precomputed and lazy pseudo-elements, used by servo. This isn't
/// a very efficient design, but is the result of servo having previously used
/// the eager pseudo map (when it was a map) for this cache.
#[cfg(feature = "servo")]
type PseudoElementCache = HashMap<PseudoElement, ComputedStyle, BuildHasherDefault<::fnv::FnvHasher>>;
#[cfg(feature = "gecko")]
type PseudoElementCache = ();

/// The styles associated with a node, including the styles for any
/// pseudo-elements.
#[derive(Clone, Debug)]
pub struct ElementStyles {
    /// The element's style.
    pub primary: ComputedStyle,
    /// A list of the styles for the element's eagerly-cascaded pseudo-elements.
    pub pseudos: EagerPseudoStyles,
    /// NB: This is an empty field for gecko.
    pub cached_pseudos: PseudoElementCache,
}

impl ElementStyles {
    /// Trivially construct a new `ElementStyles`.
    pub fn new(primary: ComputedStyle) -> Self {
        ElementStyles {
            primary: primary,
            pseudos: EagerPseudoStyles(None),
            cached_pseudos: PseudoElementCache::default(),
        }
    }

    /// Whether this element `display` value is `none`.
    pub fn is_display_none(&self) -> bool {
        self.primary.values().get_box().clone_display() == display::T::none
    }
}

/// Restyle hint for storing on ElementData.
///
/// We wrap it in a newtype to force the encapsulation of the complexity of
/// handling the correct invalidations in this file.
#[derive(Clone, Debug)]
pub struct StoredRestyleHint(RestyleHint);

impl StoredRestyleHint {
    /// Propagates this restyle hint to a child element.
    pub fn propagate(&mut self, traversal_flags: &TraversalFlags) -> Self {
        use std::mem;

        // In the middle of an animation only restyle, we don't need to
        // propagate any restyle hints, and we need to remove ourselves.
        if traversal_flags.for_animation_only() {
            if self.0.contains(RESTYLE_CSS_ANIMATIONS) {
                self.0.remove(RESTYLE_CSS_ANIMATIONS);
            }
            return Self::empty();
        }

        debug_assert!(!self.0.contains(RESTYLE_CSS_ANIMATIONS),
                      "There should not be any animation restyle hints \
                       during normal traversal");

        // Else we should clear ourselves, and return the propagated hint.
        let hint = mem::replace(&mut self.0, RestyleHint::empty());
        StoredRestyleHint(if hint.contains(RESTYLE_DESCENDANTS) {
            RESTYLE_SELF | RESTYLE_DESCENDANTS
        } else {
            RestyleHint::empty()
        })
    }

    /// Creates an empty `StoredRestyleHint`.
    pub fn empty() -> Self {
        StoredRestyleHint(RestyleHint::empty())
    }

    /// Creates a restyle hint that forces the whole subtree to be restyled,
    /// including the element.
    pub fn subtree() -> Self {
        StoredRestyleHint(RESTYLE_SELF | RESTYLE_DESCENDANTS)
    }

    /// Creates a restyle hint that forces the element and all its later
    /// siblings to have their whole subtrees restyled, including the elements
    /// themselves.
    pub fn subtree_and_later_siblings() -> Self {
        StoredRestyleHint(RESTYLE_SELF | RESTYLE_DESCENDANTS | RESTYLE_LATER_SIBLINGS)
    }

    /// Returns true if the hint indicates that our style may be invalidated.
    pub fn has_self_invalidations(&self) -> bool {
        self.0.intersects(RestyleHint::for_self())
    }

    /// Returns true if the hint indicates that our sibling's style may be
    /// invalidated.
    pub fn has_sibling_invalidations(&self) -> bool {
        self.0.intersects(RESTYLE_LATER_SIBLINGS)
    }

    /// Whether the restyle hint is empty (nothing requires to be restyled).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Insert another restyle hint, effectively resulting in the union of both.
    pub fn insert(&mut self, other: &Self) {
        self.0 |= other.0
    }

    /// Returns true if the hint has animation-only restyle.
    pub fn has_animation_hint(&self) -> bool {
        self.0.contains(RESTYLE_CSS_ANIMATIONS)
    }
}

impl Default for StoredRestyleHint {
    fn default() -> Self {
        StoredRestyleHint::empty()
    }
}

impl From<RestyleHint> for StoredRestyleHint {
    fn from(hint: RestyleHint) -> Self {
        StoredRestyleHint(hint)
    }
}

static NO_SNAPSHOT: Option<Snapshot> = None;

/// We really want to store an Option<Snapshot> here, but we can't drop Gecko
/// Snapshots off-main-thread. So we make a convenient little wrapper to provide
/// the semantics of Option<Snapshot>, while deferring the actual drop.
#[derive(Debug, Default)]
pub struct SnapshotOption {
    snapshot: Option<Snapshot>,
    destroyed: bool,
}

impl SnapshotOption {
    /// An empty snapshot.
    pub fn empty() -> Self {
        SnapshotOption {
            snapshot: None,
            destroyed: false,
        }
    }

    /// Destroy this snapshot.
    pub fn destroy(&mut self) {
        self.destroyed = true;
        debug_assert!(self.is_none());
    }

    /// Ensure a snapshot is available and return a mutable reference to it.
    pub fn ensure<F: FnOnce() -> Snapshot>(&mut self, create: F) -> &mut Snapshot {
        debug_assert!(thread_state::get().is_layout());
        if self.is_none() {
            self.snapshot = Some(create());
            self.destroyed = false;
        }

        self.snapshot.as_mut().unwrap()
    }
}

impl Deref for SnapshotOption {
    type Target = Option<Snapshot>;
    fn deref(&self) -> &Option<Snapshot> {
        if self.destroyed {
            &NO_SNAPSHOT
        } else {
            &self.snapshot
        }
    }
}

/// Transient data used by the restyle algorithm. This structure is instantiated
/// either before or during restyle traversal, and is cleared at the end of node
/// processing.
#[derive(Debug, Default)]
pub struct RestyleData {
    /// The restyle hint, which indicates whether selectors need to be rematched
    /// for this element, its children, and its descendants.
    pub hint: StoredRestyleHint,

    /// Whether we need to recascade.
    /// FIXME(bholley): This should eventually become more fine-grained.
    pub recascade: bool,

    /// The restyle damage, indicating what kind of layout changes are required
    /// afte restyling.
    pub damage: RestyleDamage,

    /// The restyle damage that has already been handled by our ancestors, and does
    /// not need to be applied again at this element. Only non-empty during the
    /// traversal, once ancestor damage has been calculated.
    ///
    /// Note that this optimization mostly makes sense in terms of Gecko's top-down
    /// frame constructor and change list processing model. We don't bother with it
    /// for Servo for now.
    #[cfg(feature = "gecko")]
    pub damage_handled: RestyleDamage,

    /// An optional snapshot of the original state and attributes of the element,
    /// from which we may compute additional restyle hints at traversal time.
    pub snapshot: SnapshotOption,
}

impl RestyleData {
    /// Computes the final restyle hint for this element.
    ///
    /// This expands the snapshot (if any) into a restyle hint, and handles
    /// explicit sibling restyle hints from the stored restyle hint.
    ///
    /// Returns true if later siblings must be restyled.
    pub fn compute_final_hint<E: TElement>(&mut self,
                                           element: E,
                                           stylist: &Stylist)
                                           -> bool {
        let mut hint = self.hint.0;

        if let Some(snapshot) = self.snapshot.as_ref() {
            hint |= stylist.compute_restyle_hint(&element, snapshot);
        }

        // If the hint includes a directive for later siblings, strip it out and
        // notify the caller to modify the base hint for future siblings.
        let later_siblings = hint.contains(RESTYLE_LATER_SIBLINGS);
        hint.remove(RESTYLE_LATER_SIBLINGS);

        // Insert the hint, overriding the previous hint. This effectively takes
        // care of removing the later siblings restyle hint.
        self.hint = hint.into();

        // Destroy the snapshot.
        self.snapshot.destroy();

        later_siblings
    }

    /// Returns true if this RestyleData might invalidate the current style.
    pub fn has_invalidations(&self) -> bool {
        self.hint.has_self_invalidations() ||
            self.recascade ||
            self.snapshot.is_some()
    }

    /// Returns true if this RestyleData might invalidate sibling styles.
    pub fn has_sibling_invalidations(&self) -> bool {
        self.hint.has_sibling_invalidations() || self.snapshot.is_some()
    }

    /// Returns damage handled.
    #[cfg(feature = "gecko")]
    pub fn damage_handled(&self) -> RestyleDamage {
        self.damage_handled
    }

    /// Returns damage handled (always empty for servo).
    #[cfg(feature = "servo")]
    pub fn damage_handled(&self) -> RestyleDamage {
        RestyleDamage::empty()
    }

    /// Sets damage handled.
    #[cfg(feature = "gecko")]
    pub fn set_damage_handled(&mut self, d: RestyleDamage) {
        self.damage_handled = d;
    }

    /// Sets damage handled. No-op for Servo.
    #[cfg(feature = "servo")]
    pub fn set_damage_handled(&mut self, _: RestyleDamage) {}
}

/// Style system data associated with an Element.
///
/// In Gecko, this hangs directly off the Element. Servo, this is embedded
/// inside of layout data, which itself hangs directly off the Element. In
/// both cases, it is wrapped inside an AtomicRefCell to ensure thread safety.
#[derive(Debug)]
pub struct ElementData {
    /// The computed styles for the element and its pseudo-elements.
    styles: Option<ElementStyles>,

    /// Restyle tracking. We separate this into a separate allocation so that
    /// we can drop it when no restyles are pending on the elemnt.
    restyle: Option<Box<RestyleData>>,
}

/// The kind of restyle that a single element should do.
pub enum RestyleKind {
    /// We need to run selector matching plus re-cascade, that is, a full
    /// restyle.
    MatchAndCascade,
    /// We need to recascade with some replacement rule, such as the style
    /// attribute, or animation rules.
    CascadeWithReplacements(RestyleHint),
    /// We only need to recascade, for example, because only inherited
    /// properties in the parent changed.
    CascadeOnly,
}

impl ElementData {
    /// Trivially construct an ElementData.
    pub fn new(existing: Option<ElementStyles>) -> Self {
        ElementData {
            styles: existing,
            restyle: None,
        }
    }

    /// Returns true if this element has a computed styled.
    pub fn has_styles(&self) -> bool {
        self.styles.is_some()
    }

    /// Returns true if this element's style is up-to-date and has no potential
    /// invalidation.
    pub fn has_current_styles(&self) -> bool {
        self.has_styles() &&
            self.restyle.as_ref().map_or(true, |r| !r.has_invalidations())
    }

    /// Returns the kind of restyling that we're going to need to do on this
    /// element, based of the stored restyle hint.
    pub fn restyle_kind(&self) -> RestyleKind {
        debug_assert!(!self.has_current_styles(), "Should've stopped earlier");
        if !self.has_styles() {
            return RestyleKind::MatchAndCascade;
        }

        debug_assert!(self.restyle.is_some());
        let restyle_data = self.restyle.as_ref().unwrap();

        let hint = restyle_data.hint.0;
        if hint.contains(RESTYLE_SELF) {
            return RestyleKind::MatchAndCascade;
        }

        if !hint.is_empty() {
            return RestyleKind::CascadeWithReplacements(hint);
        }

        debug_assert!(restyle_data.recascade,
                      "We definitely need to do something!");
        return RestyleKind::CascadeOnly;
    }

    /// Gets the element styles, if any.
    pub fn get_styles(&self) -> Option<&ElementStyles> {
        self.styles.as_ref()
    }

    /// Gets the element styles. Panic if the element has never been styled.
    pub fn styles(&self) -> &ElementStyles {
        self.styles.as_ref().expect("Calling styles() on unstyled ElementData")
    }

    /// Gets a mutable reference to the element styles, if any.
    pub fn get_styles_mut(&mut self) -> Option<&mut ElementStyles> {
        self.styles.as_mut()
    }

    /// Gets a mutable reference to the element styles. Panic if the element has
    /// never been styled.
    pub fn styles_mut(&mut self) -> &mut ElementStyles {
        self.styles.as_mut().expect("Calling styles_mut() on unstyled ElementData")
    }

    /// Borrows both styles and restyle mutably at the same time.
    pub fn styles_and_restyle_mut(&mut self) -> (&mut ElementStyles,
                                                 Option<&mut RestyleData>) {
        (self.styles.as_mut().unwrap(),
         self.restyle.as_mut().map(|r| &mut **r))
    }

    /// Sets the computed element styles.
    pub fn set_styles(&mut self, styles: ElementStyles) {
        debug_assert!(self.get_restyle().map_or(true, |r| r.snapshot.is_none()),
                      "Traversal should have expanded snapshots");
        self.styles = Some(styles);
    }

    /// Returns true if the Element has a RestyleData.
    pub fn has_restyle(&self) -> bool {
        self.restyle.is_some()
    }

    /// Drops any RestyleData.
    pub fn clear_restyle(&mut self) {
        self.restyle = None;
    }

    /// Creates a RestyleData if one doesn't exist.
    ///
    /// Asserts that the Element has been styled.
    pub fn ensure_restyle(&mut self) -> &mut RestyleData {
        debug_assert!(self.styles.is_some(), "restyling unstyled element");
        if self.restyle.is_none() {
            self.restyle = Some(Box::new(RestyleData::default()));
        }
        self.restyle.as_mut().unwrap()
    }

    /// Gets a reference to the restyle data, if any.
    pub fn get_restyle(&self) -> Option<&RestyleData> {
        self.restyle.as_ref().map(|r| &**r)
    }

    /// Gets a reference to the restyle data. Panic if the element does not
    /// have restyle data.
    pub fn restyle(&self) -> &RestyleData {
        self.get_restyle().expect("Calling restyle without RestyleData")
    }

    /// Gets a mutable reference to the restyle data, if any.
    pub fn get_restyle_mut(&mut self) -> Option<&mut RestyleData> {
        self.restyle.as_mut().map(|r| &mut **r)
    }

    /// Gets a mutable reference to the restyle data. Panic if the element does
    /// not have restyle data.
    pub fn restyle_mut(&mut self) -> &mut RestyleData {
        self.get_restyle_mut().expect("Calling restyle_mut without RestyleData")
    }
}
