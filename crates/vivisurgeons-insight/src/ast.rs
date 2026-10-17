//! What an ability *is*, as far as its text says. Nothing here knows how to
//! run: an engine would interpret these, and the spike's question is only
//! whether text can be read into them without a hole.

use crate::lex::{Pt, PtPart, Signed};

/// A counter's kind: a printed `+1/+1` (CR 122.1a), or a word ("loyalty",
/// "finality", "oil"). A tree prints either as the card says it, quoted.
#[derive(Clone, PartialEq)]
pub enum CounterKind {
    Pt(Pt),
    Named(String),
}

impl CounterKind {
    /// The kind as printed: "+1/+1", "-1/-1", "loyalty".
    pub fn text(&self) -> String {
        match self {
            CounterKind::Named(w) => w.clone(),
            CounterKind::Pt(pt) => {
                let part = |p: PtPart| match (p, pt.signed) {
                    (PtPart::N(n), true) if n < 0 => format!("-{}", -n),
                    (PtPart::N(n), true) => format!("+{n}"),
                    (PtPart::N(n), false) => n.to_string(),
                    (PtPart::X, true) => "+X".into(),
                    (PtPart::X, false) => "X".into(),
                    (PtPart::MinusX, _) => "-X".into(),
                    (PtPart::Star, _) => "*".into(),
                };
                format!("{}/{}", part(pt.power), part(pt.toughness))
            }
        }
    }

    pub fn is(&self, word: &str) -> bool {
        matches!(self, CounterKind::Named(w) if w == word)
    }
}

impl std::fmt::Debug for CounterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.text())
    }
}

impl From<&str> for CounterKind {
    fn from(w: &str) -> Self {
        CounterKind::Named(w.to_string())
    }
}

impl From<String> for CounterKind {
    fn from(w: String) -> Self {
        CounterKind::Named(w)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ability {
    Keywords(Vec<Keyword>),
    Activated {
        cost: Vec<Cost>,
        effect: Vec<Effect>,
        limits: Vec<Limit>,
    },
    Loyalty {
        change: Signed,
        effect: Vec<Effect>,
    },
    Triggered {
        trigger: Trigger,
        effect: Vec<Effect>,
        limits: Vec<Limit>,
    },
    Static(Vec<Static>),
    /// "If this card is in your opening hand, you may begin the game with it
    /// on the battlefield", "You may reveal this card from your opening
    /// hand. If you do, …": an action taken with the card from the opening
    /// hand once the mulligan process is complete, before the first turn
    /// (CR 103.6). No cost, no stack, no continuous effect: neither a
    /// static ability nor an effect ([opening-hand]).
    OpeningHandAction(OpeningHandAction),
    Spell(Vec<Effect>),
    /// A spree mode: "+ {1} — Copy target instant spell …", the cost added
    /// for choosing it.
    Spree {
        cost: Mana,
        effect: Vec<Effect>,
    },
    LevelHeader {
        from: u32,
        to: Option<u32>,
    },
    LevelPt(Pt),
    Chapter {
        chapters: Vec<u32>,
        effect: Vec<Effect>,
    },
    /// "12+ | {G}, {T}: Add {G} …": a station threshold, the ability on only
    /// while the permanent has that many charge counters.
    Station {
        at_least: u32,
        ability: Box<Ability>,
    },
    /// "1—14 | …", "15+ | …", "20 | …": a row of the results table of the
    /// die rolled before it, done if the result is from `from` to `to`
    /// (`None`: no upper end) (CR 706.3a).
    DieRow {
        from: u32,
        to: Option<u32>,
        effect: Vec<Effect>,
    },
    /// "∞ — …": an ability on only once its permanent is harnessed.
    Harnessed(Box<Ability>),
    /// "Exhaust — {3}: …", "Max speed — …": a keyword that adds rules to
    /// the ability after its dash, where an ability word adds none.
    KeywordOn {
        keyword: Prefix,
        ability: Box<Ability>,
    },
    /// An ability word or a flavour word: a label with no rules meaning.
    Labelled {
        label: String,
        ability: Box<Ability>,
    },
    /// "Tiered", then its modes, each with the additional cost paid for
    /// choosing it (CR 702.183): one mode is chosen.
    Tiered(Vec<TieredMode>),
    /// "As this enchantment enters, choose Khans or Dragons. • Khans — … •
    /// Dragons — …": the permanent has the ability whose anchor word was
    /// chosen as it entered (CR 614.12c), each linked to the choice.
    Anchored {
        what: Objects,
        options: Vec<AnchoredAbility>,
    },
}

/// "Khans — At the beginning of combat on your turn, …": an anchor word,
/// a name with no rules meaning but the choice, and the ability it gives.
#[derive(Clone, Debug, PartialEq)]
pub struct AnchoredAbility {
    pub word: String,
    pub ability: Box<Ability>,
}

/// The keywords written before a dash and an ability, each adding rules to
/// it. Exhaust, power-up, boast and forecast are said only of activated
/// abilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefix {
    /// The ability, as long as its controller's speed is 4 (CR 702.178a).
    MaxSpeed,
    /// Activate only once; it costs the permanent's mana cost less if the
    /// permanent entered this turn (CR 702.193a).
    PowerUp,
    /// Activate only once (CR 702.177a).
    Exhaust,
    /// Activate only if this creature attacked this turn and only once each
    /// turn (CR 702.142a).
    Boast,
    /// Activate only from its owner's hand, during their upkeep, once each
    /// turn (CR 702.57a–b).
    Forecast,
    /// The ability, only while this Case is solved (CR 702.169).
    Solved,
}

/// "• Fira — {2} — ~ deals 2 damage to each creature.": a flavour label,
/// the mode's additional cost, and what it does.
#[derive(Clone, Debug, PartialEq)]
pub struct TieredMode {
    pub label: String,
    pub cost: Mana,
    pub effect: Vec<Effect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyword {
    pub name: String,
    pub param: Option<KeywordParam>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeywordParam {
    Cost(Vec<Cost>),
    N(Amount),
    /// Protection from, enchant, landwalk's land, affinity for.
    Objects(Box<Objects>),
    Colors(Vec<Color>),
    /// "protection from each color that's not in your commander's color identity".
    ColorsNotIn(Palette),
    /// "protection from the color of your choice".
    Color(Palette),
    /// "Gift a card": what the promised opponent gets.
    Gift(Gift),

    /// "Splice onto Arcane {1}{R}": the spells it splices onto, and its cost.
    Splice {
        onto: Filter,
        cost: Vec<Cost>,
    },
    /// "Equip legendary creature {3}": what it may be attached to, and the
    /// cost.
    CostFor {
        what: Box<Objects>,
        cost: Vec<Cost>,
    },
    /// "Suspend 3—{1}{U}{U}", "Impending 5—{1}{B}": a number and a cost.
    NCost {
        n: Amount,
        cost: Vec<Cost>,
    },
    /// "Partner with Shabraz, the Skyshark": the one card it partners with
    /// (CR 702.124j), by name.
    PartnerWith(String),
    /// "Kicker {G} and/or {1}{U}": two costs, either or both paid.
    EitherOrBoth(Vec<Mana>),
    /// "Companion — Each creature card in your starting deck is …": the
    /// deckbuilding condition (CR 702.139).
    Condition(Box<Condition>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Gift {
    /// They draw a card.
    Card,
    ExtraTurn,
    /// "a tapped Fish", "a Food": a predefined or named token.
    Token {
        tapped: bool,
        named: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cost {
    Mana(Mana),
    /// "pay any amount of mana" (Karn, Living Legacy): mana of any kind,
    /// as much as the amount says; "that many" after it is the amount paid.
    AmountOfMana(Amount),
    Tap,
    Untap,
    Sacrifice(Objects),
    Discard(Objects),
    PayLife(Amount),
    Exile(Objects),
    RemoveCounters {
        n: Amount,
        kind: CounterKind,
        from: Objects,
    },
    PutCounters {
        n: Amount,
        kind: CounterKind,
        on: Objects,
    },
    TapUntapped(Objects),
    ReturnToHand(Objects),
    Energy(u32),
    Reveal(Objects),
    Mill(Amount),
    /// "Exert this land": it won't untap during its controller's next untap step.
    Exert(Objects),
    /// "the card's mana cost", as part of a cost built from it.
    ManaCostOf(Objects),
    /// "sacrifice an artifact or discard a card": the payer picks one.
    OneOf(Vec<Cost>),
    /// "Discard your hand".
    DiscardHand,
    /// "Pay X {E}", "pay any amount of {E}".
    PayEnergy(Amount),
    /// "{1} for each card in your hand", said after "pays": the mana once
    /// for each object counted as it is paid; after a fixed amount, "plus an
    /// additional {2} for each …".
    ManaPer {
        mana: Mana,
        per: Objects,
    },
    /// "Pay six {C}".
    ManaTimes {
        n: Amount,
        mana: Mana,
    },
    /// A keyword action paid as a cost: "Collect evidence 6", "blight 1".
    Action {
        name: String,
        n: Amount,
    },
    /// "Waterbend {8}": a keyword action with a mana cost, paid as one.
    ActionMana {
        name: String,
        mana: Mana,
    },
    /// "Remove ten oil counters from among permanents you control": split
    /// as the payer likes among them.
    RemoveCountersAmong {
        n: Amount,
        kind: CounterKind,
        among: Objects,
    },
    /// "Discard a card at random": the payer does not pick the card.
    DiscardAtRandom(Objects),
    /// "Kicker—Forage": a keyword action with no number, paid as a cost.
    ActionAlone(String),
    /// "Splice onto Arcane—An opponent gains 5 life": a cost paid by
    /// another player gaining life.
    PlayerGainsLife {
        who: Player,
        n: Amount,
    },
    /// "Remove a counter from this creature": of any kind, as the payer
    /// picks.
    RemoveAnyCounters {
        n: Amount,
        from: Objects,
    },
    /// "behold a Goblin": a keyword action done to objects, as a cost.
    ActionOn {
        name: String,
        what: Objects,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mana(pub Vec<String>);

/// What an ability's own text says about using it, after the effect.
#[derive(Clone, Debug, PartialEq)]
pub enum Limit {
    SorcerySpeed,
    /// "Activate only once each turn", "This ability triggers only once each turn".
    OncePerTurn,
    OnlyDuringYourTurn,
    OnlyIf(Condition),
    /// "Spend this mana only to cast …": a rule on the mana the ability made,
    /// and perhaps one on what it paid for ("that spell can't be countered").
    SpendOnly {
        uses: Vec<SpendUse>,
        then: Option<(Objects, Restriction)>,
    },
    /// "This ability costs {1} less to activate for each …", "… if you
    /// control a creature with a +1/+1 counter on it".
    CostChange {
        by: Mana,
        less: bool,
        per: Option<Objects>,
        cond: Option<Condition>,
    },
    /// "Activate only as an instant": no earlier than an instant could be
    /// cast, which a mana ability otherwise ignores.
    InstantSpeed,
    /// "Spend only mana of the chosen color to activate this ability."
    SpendOnlyManaOf(Palette),
    /// "Do this only once each turn": the optional part of a triggered
    /// ability, which may trigger more often than that.
    DoOnlyOncePerTurn,
    /// "Activate only once.": once for as long as the object stays the
    /// same object.
    Once,
    /// "Activate only during your upkeep."
    OnlyDuringYourUpkeep,
    /// "X can't be 0.", "X can't be greater than …": a bound on the value
    /// the controller chooses for X (CR 107.3a).
    X(XLimit),
    /// "This ability can't be copied."
    CantBeCopied,
}

/// A bound on a chosen X (CR 107.3a).
#[derive(Clone, Debug, PartialEq)]
pub enum XLimit {
    /// "X can't be 0."
    NotZero,
    /// "X can't be greater than the number of players in the game."
    AtMost(Amount),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpendUse {
    Cast(Objects),
    ActivateAbilityOf(Objects),
    /// "to activate abilities": of any object.
    ActivateAny,
    /// "on costs that contain {X}": any cost with an {X} in it, a spell's
    /// or an ability's (CR 107.3).
    CostsWithX,
}

/// A die result's parity: "each odd result", "each even result".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parity {
    Odd,
    Even,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Amount {
    /// "your starting life total" (CR 119.1).
    StartingLifeTotal(Player),
    /// "with mana value 4, 5, or 6", "2 or 3": any one of the values, in a
    /// comparison for equality.
    AnyOf(Vec<Amount>),
    N(u32),
    X,
    ThatMany,
    CountOf(Box<Objects>),
    /// "the number of nontoken creatures you had enter the battlefield under
    /// your control this turn" (Gyome), "for each land that entered the
    /// battlefield under your control this turn": how many objects that
    /// were `what` as they entered entered under `under`'s control this turn,
    /// counted by the entries, so one that has since left the battlefield,
    /// stopped being `what` or changed control still counts (the Scryfall
    /// rulings on Gyome and Hobgoblin Bandit Lord) ([entries-this-turn]).
    /// `what` holds no `Post::EnteredUnderThisTurn`, which is the filter of
    /// a permanent now on the battlefield.
    EntriesThisTurn {
        what: Box<Objects>,
        under: Player,
    },
    PowerOf(Box<Objects>),
    ToughnessOf(Box<Objects>),
    ManaValueOf(Box<Objects>),
    LifeLost,
    /// "the damage dealt this way", "that damage": what an earlier event dealt.
    Damage,
    /// "triple that damage".
    Thrice(Box<Amount>),
    /// "the amount of mana spent to cast this creature".
    ManaSpentToCast(Box<Objects>),
    /// "the amount of {S} spent to cast this spell": the mana spent to cast it
    /// that a snow source produced (CR 107.4h).
    SnowManaSpentToCast(Box<Objects>),
    /// "the number of times it's been cast from the command zone this game".
    TimesCastFrom {
        what: Box<Objects>,
        zone: Zone,
    },
    /// "each time you've cast your commander from the command zone this
    /// game": the casts that player made of such objects from that zone.
    /// "a commander" counts any commander that player cast, theirs or not.
    TimesCast {
        who: Player,
        what: Box<Objects>,
        zone: Zone,
    },
    /// "you've cycled a card named ~ four or more times this game"
    /// (Yidaro): how many times the event has happened this game.
    TimesThisGame(Box<Event>),
    /// "an opponent lost 2 or more life this turn".
    LifeLostThisTurn(Player),
    /// "the amount of life you gained this turn".
    LifeGainedThisTurn(Player),
    /// "the number of cards you've drawn this turn".
    CardsDrawnThisTurn(Player),
    /// "the number of differently named lands you control": how many names
    /// among them.
    DifferentlyNamed(Box<Objects>),
    /// "an opponent was dealt 7 or more damage this turn".
    DamageDealtThisTurn(Player),
    /// "each 1 damage prevented this way".
    DamagePrevented,
    /// "each odd result": the results of the dice just rolled that are odd
    /// (or even), one per die (CR 706.2: the result after modifiers).
    RolledResults(Parity),
    /// "each evidence vote": the votes cast for that choice (CR 701.38), one
    /// per vote.
    Votes(String),
    /// "each time it has attacked this turn".
    TimesAttackedThisTurn(Box<Objects>),
    /// "up to two": the player picks any number to that bound.
    UpTo(Box<Amount>),
    Twice(Box<Amount>),
    /// "that many plus one".
    Plus(Box<Amount>, u32),
    /// "X cards plus an additional card for each time they've cast a
    /// commander from the command zone this game": the first amount, and
    /// one more for each the second counts.
    PlusEach(Box<Amount>, Box<Amount>),
    /// "the greatest power among creatures you control".
    Greatest {
        stat: Stat,
        among: Box<Objects>,
    },
    /// "your devotion to black".
    Devotion {
        who: Player,
        to: Palette,
    },
    /// "the number of colors in your commander's color identity".
    NumberOfColors(Box<Palette>),
    /// "the greatest number of cards a player discarded this way".
    GreatestDiscardedThisWay,
    /// "for each time it was kicked".
    TimesKicked,
    /// "the total power of creatures you control".
    Total {
        stat: Stat,
        of: Box<Objects>,
    },
    /// "one or two": any number from the first to the second.
    Between(u32, u32),
    /// "two additional cards": more than the rules would give anyway.
    Additional(Box<Amount>),
    /// "its base power": power before any effect changes it.
    BasePowerOf(Box<Objects>),
    /// "four or more card types among cards in your graveyard".
    NumberOfCardTypes(Box<Objects>),
    /// "the result" of a die rolled before.
    DieResult,
    /// "that damage": the damage an event the trigger or replacement names
    /// dealt, where "the damage dealt this way" is this ability's own.
    ThatDamage,
    /// "your life total".
    LifeTotal(Player),
    /// "the number of basic land types among lands you control": domain.
    NumberOfBasicLandTypes(Box<Objects>),
    /// "the number of colors of mana spent to cast this spell": converge.
    ColorsOfManaSpentToCast(Box<Objects>),
    /// "half X, rounded down", "the top half of their library, rounded up".
    Half {
        of: Box<Amount>,
        round_up: bool,
    },
    /// "a third of their life, rounded up".
    Third {
        of: Box<Amount>,
        round_up: bool,
    },
    /// "less than or equal to half your starting life total": compared
    /// exactly, so not rounded (CR 107.1a rounds only a number the
    /// effect generates).
    HalfUnrounded(Box<Amount>),
    /// "any amount of life": as much as the payer picks.
    AnyAmount,
    /// "the amount of {E} paid this way".
    EnergyPaidThisWay,
    /// "that number plus 1": the number the sentence just named.
    ThatNumber,
    /// "if you have fewer than seven cards in hand, draw cards equal to the
    /// difference": how far apart the two sides of the comparison the
    /// condition before it tested are ([the-difference]).
    TheDifference,
    /// "three minus X", "that many minus one".
    Minus(Box<Amount>, Box<Amount>),
    /// "for every seven cards in your graveyard": the count, divided by n
    /// and rounded down.
    Every {
        n: u32,
        of: Box<Objects>,
    },
    /// "your speed": 0 for a player with none (CR 702.179f).
    Speed(Player),
    /// "Whenever you cast a spell with {X} in its mana cost, put X +1/+1
    /// counters on ~": the X of that object's mana cost (CR 107.3e), not
    /// one this ability's controller chooses (CR 107.3f).
    XOf(Box<Objects>),
    /// "When this creature is turned face up, all creatures get -X/-X"
    /// (Bane of the Living, Morph {X}{B}{B}): the X chosen for the morph or
    /// disguise cost paid to turn this permanent face up (CR 107.3d; Gatherer
    /// ruling 2004-10-04), not one the controller chooses again.
    XTurnedFaceUp,
}

/// A set of colors a rule draws from, named by what decides it.
#[derive(Clone, Debug, PartialEq)]
pub enum Palette {
    Listed(Vec<Color>),
    /// "the chosen color": the one an "as ~ enters, choose a color" named.
    Chosen,
    /// "that color": the color an earlier clause named, as the color of
    /// each pass of a "for each color" or the one a trigger mentioned.
    That,
    CommanderIdentity,
    /// "the exiled card's colors", "among legendary creatures you control".
    ColorsOf(Box<Objects>),
    /// "that a land an opponent controls could produce".
    CouldProduce(Box<Objects>),
    /// "the color of your choice": one color, picked as the effect happens.
    ChoiceOf(Player),
    /// "any type that land produced": what an earlier tap made.
    Produced(Box<Objects>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
    Multicolored,
    Monocolored,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Player {
    You,
    Target,
    TargetOpponent,
    Each,
    EachOpponent,
    EachOtherPlayer,
    AnOpponent,
    APlayer,
    /// "another player" (Firemane Commando): any player but you.
    Another,
    /// "if that opponent has more life than another of your opponents"
    /// (Breena, the Demagogue): one of your opponents other than the player
    /// named, any one of them.
    AnotherOpponentThan(Box<Player>),
    That,
    DefendingPlayer,
    ControllerOf(Box<Objects>),
    OwnerOf(Box<Objects>),
    Chosen,
    Active,
    /// "you and target opponent each": every one of these players.
    EachOf(Vec<Player>),
    /// "each player who can't": those of them who could not do what an
    /// earlier effect asked.
    WhoCant(Box<Player>),
    /// "enchanted player": the one a Curse is attached to.
    Enchanted,
    /// "each opponent attacking that player".
    Attacking {
        who: Box<Player>,
        attacked: Box<Player>,
    },
    /// "that attacking player": the player whose attack the trigger saw,
    /// where "that opponent" is the one attacked (Combat Calligrapher).
    AttackingPlayer,
    /// "each player who searched a library this way".
    DidThisWay {
        who: Box<Player>,
        did: Box<Event>,
    },
    /// "up to one target player": a target the controller may leave unchosen.
    UpToOneTarget,
    /// "any number of target players' graveyards".
    AnyNumberOfTarget,
    /// "Any number of target opponents each sacrifice a creature …" (Will of
    /// the Abzan).
    AnyNumberOfTargetOpponents,
    /// "two target players each draw a card", "up to two target players each
    /// manifest two cards from their hands" (Kozilek, the Broken Reality):
    /// that many target players, or up to that many, each doing it.
    NTarget {
        n: u32,
        up_to: bool,
    },
    /// "that player or that planeswalker's controller": whichever one the
    /// earlier choice made it.
    OneOf(Vec<Player>),
    /// "target player who was dealt combat damage by ~ this turn", "each
    /// opponent who attacked you … this turn": the players `who` names, those
    /// of them each post holds of.
    Who {
        who: Box<Player>,
        posts: Vec<Post>,
    },
    /// "Those players" after "For each permanent destroyed this way, its
    /// controller …": every player the loop before named, all at once.
    Those(Box<Player>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Objects {
    pub quant: Quant,
    pub filter: Filter,
}

/// The card itself (`Quant::SelfRef`), the one way the grammar builds it.
pub fn self_ref() -> Objects {
    self_ref_in(Vec::new())
}

/// The card itself where the text says which zone it is in: "return this
/// card from your graveyard to your hand". The zone is no descriptor (CR
/// 700.7 is about the noun), so it stays; whether it belongs on the
/// effect rather than the object is open ([self-reference]).
pub fn self_ref_in(zone: Vec<Post>) -> Objects {
    Objects {
        quant: Quant::SelfRef,
        filter: Filter {
            posts: zone,
            ..Filter::default()
        },
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Quant {
    Target {
        n: Amount,
        up_to: bool,
    },
    AnyTarget {
        n: Amount,
    },
    /// "any other target": a creature, player, planeswalker or battle other
    /// than the object the sentence named before it (its source, or an
    /// earlier target).
    AnyOtherTarget,
    Each,
    All,
    One,
    N(Amount),
    /// "the top card of each player's library": that many from each
    /// player's zone, so as many sets as there are players.
    NFromEach(Amount),
    OneOrMore,
    AnyNumber,
    /// "any number of target creatures": as many targets as the player
    /// picks, with no bound, where "up to X target" has one.
    AnyNumberOfTarget,
    /// "you control a prime number of lands" (Zimone, All-Questioning): as
    /// many as some prime, 2, 3, 5, 7 ….
    PrimeNumber,
    /// "the last creature card exiled with it" (Duplicant): of the objects
    /// the filter names, the one that became one most recently (its
    /// ruling: the exiled card with the latest timestamp).
    Last,
    /// "any number of other target creatures" (Kinetic Ooze): as many as the
    /// player picks, each other than this object.
    AnyNumberOfOtherTarget,
    UpTo(Amount),
    /// "another creature": one object other than the card itself. Where the text means other than some other
    /// object (the trigger's, a clause's target, each of a set in turn) the
    /// tree says `One` with `Adj::OtherThan(that object)` instead
    /// ([other-than-trigger], [other-than-subject], [other-than-named-before]).
    Another,
    AnotherTarget,
    /// "a second target permanent": another use of "target", which may
    /// choose the same object as the first (CR 115.3), where "another
    /// target" may not.
    SecondTarget,
    /// "up to one other target creature": targets, none of them this object,
    /// whatever kinds the filter lists.
    OtherTarget {
        n: Amount,
        up_to: bool,
    },
    /// The card itself, however the text names it: "~", "this creature",
    /// "this Aura", "this spell", "this card", "this token". "This
    /// [descriptor]" is that one object "even if it isn't the appropriate
    /// quality at the time" (CR 700.7), so the noun is Oracle's shorthand
    /// for "this card" and never a condition: the filter holds no alts and
    /// no kind. Build it with `self_ref` ([self-reference]).
    SelfRef,
    /// The card's name inside a quoted ability it grants: the source of
    /// the granting ability, and no other object (CR 201.5a). Shifting
    /// Shadow's enchanted creature has "… attach ~ to it": ~ is the Aura,
    /// while "this creature" in the same quote is the creature that has
    /// the ability (`SelfRef`).
    Granter,
    /// The object whose event triggered the ability: Valakut's "other
    /// Mountains" are other than "that new one" (its ruling), and Orah's
    /// "card with lesser mana value" is lesser than the Cleric that died.
    Triggering,
    /// "it", "them", "that creature", "those cards": an earlier noun. In
    /// the clauses of a `ForEach`, the object of that pass ("For each
    /// creature you control, put a +1/+1 counter on it").
    Antecedent,
    /// A reference back said before a trailing "for each", so an earlier
    /// noun and never the object each pass counts: "put a +1/+1 counter on
    /// it for each creature your opponents control" is the token created
    /// before. Where the referent rule knows the object it is `Triggering`
    /// or `SelfRef` instead ("it gets +1/+0 … for each other attacking
    /// creature").
    BeforeEach,
    /// A reference back in an "if", "unless" or "as long as" said after its
    /// clause: the one object that clause names, which the tree holds after
    /// the condition. "Destroy target creature if it's tapped" tests the
    /// target, not the spell that triggered the ability or an earlier
    /// noun ([trailing-if]).
    ClauseObject,
    /// "Exile up to one target artifact you control and up to one target
    /// creature you control. Then return them", "tap it and up to one target
    /// creature …. They don't untap": every object a verb said once named
    /// just before, which no one `Objects` names, so a bare `Antecedent`
    /// would be the last of them alone ([listed-objects]).
    Listed,
    /// Each one of a set in turn, said by "its" or "that creature": in a
    /// comparison an object is tested by ("creatures you control each with
    /// power greater than its base power" compares each creature with its
    /// own base power) and in an amount worked out for each object an
    /// effect acts on ("each creature you control deals damage equal to its
    /// power"). Where the tree can tell, "each in turn" is this, never a
    /// bare `Antecedent`.
    Itself,
    /// "Create a token that's a copy of …. Sacrifice it at the beginning of
    /// the next end step", "Create two 1/1 … tokens. They gain haste": the
    /// tokens the effect before created, which no `Objects` in the tree
    /// names, so a bare `Antecedent` would point past them to the copied
    /// creature.
    Created,
    /// "Counter target activated ability from an artifact source and destroy
    /// that artifact if it's on the battlefield": the object that generated
    /// the ability (CR 113.7), which the ability named as its source.
    SourceOf(Box<Objects>),
    /// "you may copy it. If you do, those spells gain wither": the spell the
    /// effect before copied and its copies, each a spell (CR 707.10).
    OriginalAndCopies,
    /// "the exiled card", "the token" in an ability that exiled or created
    /// nothing: the object a linked ability of this object exiled or
    /// created (CR 607.1, 607.2a), which the filter's first post names
    /// (`ExiledWith(SelfRef)`, `CreatedWith(SelfRef)`). No reference back:
    /// nothing before it in the ability names it.
    Linked,
    /// "This ability costs {1} less to activate if it targets a creature
    /// with power 3 or less": the ability the text is part of, which is no
    /// card (CR 113.1).
    ThisAbility,
    /// "Put both cards onto the battlefield": the two objects the clauses
    /// before it named, one each (Sword of Hearth and Home's exiled
    /// creature card and the land card found), where "them" is one plural.
    Both,
    Enchanted,
    Equipped,
    /// A bare plural: "Creatures you control get +1/+1".
    Plural,
    Other,
    AtLeast(Amount),
    AtMost(Amount),
    /// "the rest", "the other": what an earlier choice left.
    Rest,
    /// "this creature or another Ally you control": itself, as the first
    /// object says it, or one the filter names.
    SelfOrAnother(Box<Objects>),
    /// "this creature and other Vampire creatures you control": the one
    /// object, and every other the filter names.
    WithOthers(Box<Objects>),
    /// "this creature or a commander you control": itself, or any one the
    /// filter names, itself not excluded as "another" would.
    SelfOrA(Box<Objects>),
    /// "this creature and land creatures you control": the one object, and
    /// every one the filter names, itself not excluded as "other" would.
    SelfAnd(Box<Objects>),
    /// "your commander": the one a player has, by a possessive.
    Possessive(Player),

    /// "their first noncreature spell each turn".
    NthEachTurn {
        n: u32,
        of: Player,
    },
    /// "your first spell during each opponent's turn", "… during each of
    /// your turns": the nth in each turn of `turns`, none in the others.
    NthDuringTurns {
        n: u32,
        of: Player,
        turns: Player,
    },
    /// "the next spell you cast this turn".
    Next,
    /// "additional spells": more than those already counted.
    Additional,
    /// "their first spell of the game".
    NthOfGame {
        n: u32,
        of: Player,
    },
    /// "target nonland permanent and all other permanents with the same
    /// name as that permanent": the one object, and every other the filter
    /// names.
    WithAllOthers(Box<Objects>),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Filter {
    /// An or of ands: "white or black creature" is `[[white, creature], [black, creature]]`.
    pub alts: Vec<Vec<Adj>>,
    /// What kind of object: "card", "spell", "token". `None` means a permanent
    /// named by its type, as in "target creature".
    pub kind: Option<Noun>,
    pub posts: Vec<Post>,
    /// "spell or nonland permanent an opponent controls": a second kind of
    /// object the same slot may hold.
    pub or: Option<Box<Filter>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Adj {
    Color(Color),
    /// "a permanent spell": one that will enter the battlefield as a
    /// permanent, an artifact, battle, creature, enchantment, or
    /// planeswalker spell (CR 110.4b).
    Permanent,
    Non(String),
    NonColor(Color),
    Type(String),
    Supertype(String),
    Subtype(String),
    State(State),
    /// "activated ability", "triggered ability".
    Ability(AbilityKind),
    /// "other" said of an object other than the card itself: Valakut's
    /// "five other Mountains" are other than the one that entered.
    /// `State::Other` is other than the card.
    OtherThan(Box<Objects>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbilityKind {
    Activated,
    Triggered,
    /// "a loyalty ability": an activated ability with a loyalty symbol in its
    /// cost (CR 606.3).
    Loyalty,
    /// "an exhaust ability": an activated ability with exhaust (CR 702.177a).
    Exhaust,
    /// "a cycling ability": cycling or any typecycling (CR 702.29f).
    Cycling,
    /// "a mana ability" (CR 605.1).
    Mana,
    /// "equip abilities": the equip keyword ability (CR 702.6a).
    Equip,
    /// "ninjutsu abilities": the ninjutsu keyword ability (CR 702.49a).
    Ninjutsu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Tapped,
    Untapped,
    Attacking,
    Blocking,
    AttackingOrBlocking,
    Blocked,
    Unblocked,
    Token,
    Nontoken,
    FaceDown,
    Other,
    /// Equipped, enchanted by an Aura its controller controls, or with a
    /// counter on it.
    Modified,
    /// Artifact, legendary or Saga.
    Historic,
    /// "Commander creatures you control": a commander, by a word before its type.
    Commander,
    /// "a face-up artifact card you own in exile".
    FaceUp,
    /// "if this creature isn't prepared": a preparation card whose copy of
    /// its inset spell may be cast (CR 722.3).
    Prepared,
    /// "attacks while saddled": a designation until end of turn (CR 702.171b).
    Saddled,
    /// "all enchanted creatures", "a creature you control that's enchanted":
    /// with an Aura attached (CR 303.4), any Aura; apart from
    /// `Quant::Enchanted`, the one this Aura enchants.
    Enchanted,
    /// "that's enchanted or equipped": with an Equipment attached (CR 301.5).
    Equipped,
    /// "a land or double-faced card": a card with a Magic card face on each
    /// side, or half an oversized face on one (CR 712.1).
    DoubleFaced,
    /// "transformed permanents": double-faced permanents with their back
    /// face up (CR 712.8e).
    Transformed,
    /// "target permanent or suspended card": a card in exile that has
    /// suspend and a time counter on it (CR 702.62b).
    Suspended,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Noun {
    Permanent,
    Spell,
    Card,
    Token,
    Player,
    Opponent,
    Ability,
    SpellOrAbility,
    Source,
    PermanentCard,
    Commander,
    /// A copy of a spell: "the copy".
    Copy,
    /// "burden counter", or any kind when `None`: "those counters".
    Counter(Option<CounterKind>),
    /// "one pile": a pile an earlier separation made.
    Pile,
}

/// A card's name as printed: a string any object may share (CR 201.2a),
/// not the object.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintedName(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum Post {
    /// "a spell that's exactly two colors", "a nonland permanent that isn't
    /// exactly two colors" (with `Not`): its number of colors (CR 105.2).
    ExactlyColors(u32),
    /// "Whenever you cast a legendary spell from your hand": the zone a spell
    /// was cast from, which it left as it was cast (CR 601.2a); a spell is
    /// on the stack, so it is in no hand.
    CastFrom(Zone),
    /// "activated ability from an artifact source": what the ability's
    /// source is (CR 113.7).
    FromSource(Vec<Adj>),
    /// "up to three target creature cards with total mana value 3 or less":
    /// a bound on the sum over the objects chosen, not on each.
    Total(Compare),
    Controller(Player),
    NotControlledBy(Player),
    Owner(Player),
    In(Zone),
    With(Compare),
    /// "with base power 0": its power before any effect changes it (CR
    /// 208.4). Only power or toughness.
    Base(Compare),
    /// "with base power and toughness 1/1".
    BasePt(Pt),
    /// "that's enchanted by an Aura you control", "enchanted by two or more
    /// Auras": attached to by such an Aura (CR 303.4).
    EnchantedBy(Box<Objects>),
    WithKeyword(String),
    WithoutKeyword(String),
    /// "with a +1/+1 counter on it", or of any kind when `None`: "with
    /// counters on them".
    WithCounter(Option<CounterKind>),
    /// "a spell named ~", "a card named ~": the card's own printed name,
    /// a string, not the object: Approach of the Second Sun counts another
    /// spell with that name (CR 201.2a). Inside a quoted ability it is still the printed name
    /// (Rekindling Phoenix's Elemental returns "a card named Rekindling
    /// Phoenix") ([named-self]). "Differently named" compares objects'
    /// names, `Amount::DifferentlyNamed`.
    Named(PrintedName),
    /// "spells your opponents cast that target this creature".
    Targets(Box<Objects>),
    /// "each spell you cast that's red or green".
    ThatIs(Filter),
    Attacking,
    Blocking,
    AttachedToIt,
    /// "that was attached to it", "all Auras and Equipment that were attached
    /// to it": attached to the object named before, until the event or
    /// effect before ended that (a leaves-the-battlefield trigger looks back
    /// in time, CR 603.10a).
    WasAttachedToIt,
    /// "each Aura you controlled that was attached to it" (Hateful
    /// Eidolon): controlled by that player then.
    ControlledThen(Player),
    /// "each Aura you control that's attached to a creature": attached to
    /// one of those objects.
    AttachedTo(Box<Objects>),
    /// "of the chosen type", "of that type".
    OfChosenType,
    /// "of the chosen color", "of that color".
    OfChosenColor,
    /// "of that type", "of that color": the one an earlier clause named.
    OfThatType,
    OfThatColor,
    /// "that aren't of the chosen type".
    Not(Box<Post>),
    /// "all permanents except for lands and tokens", "all creatures except
    /// creatures chosen this way": none of what the filter describes.
    Except(Filter),
    /// "that entered this turn".
    EnteredThisTurn,
    /// "that entered the battlefield under your control this turn": under
    /// whose control it entered, whoever controls it now.
    EnteredUnderThisTurn(Player),
    /// "from among the nonland cards exiled this way".
    FromAmong(Box<Objects>),
    /// "with the greatest mana value or tied for the greatest mana value".
    Greatest {
        stat: Stat,
        or_tied: bool,
    },
    /// "who drew a card this way", "that had a counter put on them this way".
    DidThisWay(Box<Event>),
    /// "with the same name", "with the same name as one another".
    SameName,
    /// "that each have a different mana value".
    DifferentValues(Stat),
    /// "that are one or more colors".
    OneOrMoreColors,
    /// "that are attacking you and/or planeswalkers you control".
    AttackingAnyOf(Vec<Recipient>),
    /// "a card with the chosen name".
    WithChosenName,
    /// "a permanent they control of each permanent type": one per type.
    OfEachPermanentType,
    /// "other than ~", "other than this creature": the card, not an object
    /// of its own: a trigger's event still names one object. The noun it
    /// was named by is no condition, as with `Quant::SelfRef`.
    OtherThanSelf,
    Cast(Player),
    FromAmongThem,
    ExiledWith(Box<Objects>),
    /// "the exiled cards used to craft it": the cards exiled to pay the
    /// craft cost that put this permanent onto the battlefield (CR
    /// 702.167c).
    UsedToCraft(Box<Objects>),
    /// "that shares a creature type with it"; with no object, with each other.
    SharesType {
        card_type: String,
        with: Option<Box<Objects>>,
    },
    WithBasicLandType,
    /// "with a single target".
    WithSingleTarget,
    /// "of their choice": who picks among the objects named.
    ChosenBy(Player),
    /// "spell you've cast this turn": one already cast.
    CastThisTurn(Player),
    /// "the next spell you cast this turn": one not cast yet.
    WillCastThisTurn(Player),
    /// "with mana cost {0} or {1}".
    ManaCost(Vec<Mana>),
    /// "a card with a cycling ability", "that has a cycling ability": one
    /// with an ability of that kind (for cycling, any typecycling too, CR
    /// 702.29f).
    WithAbilityOf(AbilityKind),
    /// "with mana value of the chosen quality", "with power of that
    /// quality": whose value has the parity chosen (odd or even).
    OfChosenParity(Stat),
    /// "a spell with {X} in its mana cost": one whose mana cost has the
    /// symbol (CR 107.3), whatever X was chosen as.
    XInManaCost,
    /// "a spell that's one or more colors without {X} in its mana cost".
    NoXInManaCost,
    AttackingPlayer(Player),
    /// "counter on ~".
    On(Box<Objects>),
    /// "the exiled card": an object an earlier verb picked out.
    Marked(Marked),
    PutOntoBattlefieldWith(Box<Objects>),
    /// "the token" in an ability that created none: one a linked ability of
    /// that object created (CR 607.1).
    CreatedWith(Box<Objects>),
    /// "cards they exiled this way": marked by an earlier verb of one player.
    MarkedBy {
        how: Marked,
        by: Player,
    },
    /// "cards in your hand drawn this turn".
    DrawnThisTurn,
    /// "each opponent who doesn't": who did not do what an earlier "may" offered.
    WhoDoesnt,
    /// "each opponent who can't": who could not do what an earlier effect asked.
    WhoCant,
    /// "that aren't on the battlefield".
    NotIn(Zone),
    /// "lands with different names".
    DifferentNames,
    /// "with power or toughness 1 or less": either one meets the bound.
    PowerOrToughness {
        op: Op,
        value: Amount,
    },
    /// "creatures that died this turn".
    DiedThisTurn,
    /// "creatures that died under your control this turn": that player
    /// controlled each as it died (CR 603.10a), where `Controller` would
    /// say who controls it now.
    DiedUnderControlThisTurn(Player),
    /// "creature that crewed it this turn": tapped this turn to pay that
    /// Vehicle's crew cost (CR 702.122b); `saddle`, "that saddled it this
    /// turn", its saddle cost (CR 702.171c).
    CrewedThisTurn {
        what: Box<Objects>,
        saddle: bool,
    },
    /// "creature cards in your graveyard that were put there from the
    /// battlefield this turn": put into that graveyard from the battlefield
    /// this turn, creatures or not.
    PutThereFromBattlefieldThisTurn,
    /// "a nonland card from it": from the hand or pile an earlier verb named.
    From(Box<Objects>),
    /// "Target opponent reveals their hand. You choose a nonland card from
    /// it": that player's hand, which the sentence before revealed or
    /// looked at, said by pronoun. A hand is in no `Objects`, so `From` a
    /// reference back would point at an object named before it.
    FromHand(Player),
    /// "with the greatest mana value among creatures and planeswalkers they
    /// control": greatest within those, not on the battlefield.
    GreatestAmong {
        stat: Stat,
        among: Box<Objects>,
    },
    /// "a permanent dealt damage this way".
    DealtDamageThisWay,
    /// "a creature dealt damage by this creature this turn", "target
    /// creature that was dealt damage this turn", "target player who was
    /// dealt combat damage by ~ this turn": damage dealt to it this turn,
    /// by `by` when said, combat damage only when `combat`.
    WasDealtDamageThisTurn {
        by: Option<Box<Objects>>,
        combat: bool,
    },
    /// "whose controller was dealt combat damage by this creature this
    /// turn": the post holds of the object's controller.
    ControllerIs(Box<Post>),
    /// "target creature that dealt damage this turn", "creatures that dealt
    /// damage to you this turn": damage it dealt this turn, to `to` when
    /// said.
    DealtDamageThisTurn(Option<Player>),
    /// "with seven or more cards in their graveyard": said of a player, who
    /// has at least `n` of what `what` counts.
    HasAtLeast {
        n: u32,
        what: Holding,
    },
    /// "who attacked you or a planeswalker you control this turn": the
    /// player attacked one of these this turn (CR 508.1a).
    AttackedThisTurn(Vec<Recipient>),
    /// "permanents that were cast this turn": by any player.
    WasCastThisTurn,
    /// "each opponent who lost life this turn".
    LostLifeThisTurn,
    /// "the number of opponents who control a creature with power 4 or
    /// greater" (Summon: Yojimbo): a player who controls one or more of
    /// the objects.
    WhoControls(Box<Objects>),
    /// "a land that could produce {C}".
    CouldProduce(Mana),
    /// "that is snow or could produce {C}": any one of these holds.
    Either(Vec<Post>),
    /// "for each player who chose money" (Master of Ceremonies): the
    /// players whose `Choice::Words` pick was this word.
    ChoseWord(String),
    /// "unless it's only the color its controller chose" (Selective
    /// Obliteration): exactly one color, the one that player chose (CR
    /// 105.2a: a monocolored object is exactly one of the five colors).
    OnlyColorChosenBy(Player),
    /// "with four or more card types among them".
    CardTypesAmongAtLeast(u32),
    /// "with the same name as that card".
    SameNameAs(Box<Objects>),
    /// "cards exiled from their hand this way".
    MarkedFrom {
        how: Marked,
        from: Zone,
    },
    /// "with that name": the name an earlier choice named, as "that color"
    /// is to "the chosen color".
    WithThatName,
    /// "[with mana value 2 or less]": words cleave removes (CR 702.148).
    Cleave(Vec<Post>),
    /// "other than a basic land card".
    OtherThan(Filter),
    /// "cards you don't own".
    NotOwnedBy(Player),
    /// "a creature named Hanweir Garrison": another card, by name.
    NamedCard(String),
    /// "each card you've cycled or discarded this turn".
    DoneThisTurn {
        who: Player,
        verbs: Vec<PastVerb>,
    },
}

/// What a player did to a card this turn, as a post names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PastVerb {
    Cycled,
    Discarded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marked {
    Exiled,
    Chosen,
    Revealed,
    Sacrificed,
    Copied,
    Returned,
    Destroyed,
    /// "each card they find": found by a search.
    Found,
    Tapped,
    Milled,
    Discarded,
    /// "for each counter removed this way" (Nexus Mentality): removed by
    /// the effect before.
    Removed,
    /// "If an Equipment is put onto the battlefield this way" (Armored
    /// Skyhunter): put there by the effect before.
    PutOntoBattlefield,
    /// "For each card manifested this way" (Kozilek, the Broken Reality):
    /// turned face down as a 2/2 by the effect before (CR 701.40a).
    Manifested,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Compare {
    pub stat: Stat,
    pub op: Op,
    pub value: Amount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    Power,
    Toughness,
    ManaValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Eq,
    Le,
    Ge,
    Lt,
    Gt,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Zone {
    Hand(Player),
    Graveyard(Player),
    AGraveyard,
    /// "up to two target cards from a single graveyard": every one of the
    /// objects from the same graveyard.
    SingleGraveyard,
    /// "searches a library": any one.
    ALibrary,
    Library(Player),
    TopOfLibrary(Player),
    BottomOfLibrary(Player),
    OwnersHand,
    OwnersGraveyard,
    OwnersLibraryTop,
    OwnersLibraryBottom,
    /// "into its owner's library seventh from the top".
    OwnersLibraryNthFromTop(u32),
    Battlefield,
    Exile,
    Stack,
    CommandZone,
    Anywhere,
    /// "anywhere other than the battlefield".
    AnywhereOtherThan(Box<Zone>),
    /// "from outside the game" (CR 400.11).
    OutsideTheGame,
    /// "in graveyards and libraries": any zone of these kinds.
    AnyOf(Vec<Zone>),
    /// "on their choice of the top or bottom of their library": either end,
    /// as the player who puts it there picks.
    TopOrBottomOfLibrary(Player),
    /// "your starting deck", which a companion condition is about.
    StartingDeck(Player),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Duration {
    UntilEndOfTurn,
    ThisTurn,
    /// "this combat": to the end of the combat phase (CR 506.1), as a
    /// delayed trigger's "Whenever that creature deals combat damage to a
    /// player this combat" lasts.
    ThisCombat,
    UntilYourNextTurn,
    WhileSelfOnBattlefield,
    Permanent,
    /// "for the rest of the game".
    RestOfGame,
    /// "for as long as they remain exiled", "for as long as it has a vow
    /// counter on it".
    ForAsLongAs(Box<Condition>),
    /// "until the end of your next turn": through your next turn, where
    /// "until your next turn" ends as it begins.
    UntilEndOfYourNextTurn,
    /// "until this card is cast from exile".
    UntilEvent(Box<Event>),
}

/// When an [`Effect::AnyTime`] action may be taken.
#[derive(Clone, Debug, PartialEq)]
pub enum AnyTimeWindow {
    /// "any time you could activate a mana ability" (CR 605.3a).
    ManaAbility,
    /// "any time you could cast an instant": whenever you have priority
    /// (CR 117.1a).
    Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Draw {
        who: Player,
        n: Amount,
    },
    Discard {
        who: Player,
        what: Objects,
        random: bool,
    },
    DiscardHand(Player),
    /// "your life total becomes 15", "… becomes half your starting life
    /// total, rounded up": the player gains or loses the life needed to end
    /// with it (CR 119.5).
    LifeTotalBecomes {
        who: Player,
        n: Amount,
    },
    Damage {
        source: Option<Objects>,
        n: Amount,
        to: Vec<Recipient>,
    },
    Destroy {
        what: Objects,
        no_regen: bool,
    },
    Exile(Objects),
    Move {
        what: Objects,
        from: Option<Zone>,
        to: Zone,
        tapped: bool,
        transformed: bool,
        /// "under your control", "under its owner's control".
        under: Option<Player>,
        order: Option<Order>,
    },
    Counter {
        what: Objects,
        unless: Option<(Player, Vec<Cost>)>,
    },
    Create {
        who: Player,
        n: Amount,
        token: Token,
    },
    PutCounters {
        n: Amount,
        kind: CounterKind,
        on: Objects,
    },
    RemoveCounters {
        n: Amount,
        kind: CounterKind,
        from: Objects,
    },
    Modify {
        what: Objects,
        pt: Option<Pt>,
        gains: Vec<Keyword>,
        dur: Duration,
    },
    GainLife {
        who: Player,
        n: Amount,
    },
    LoseLife {
        who: Player,
        n: Amount,
    },
    AddMana(ManaAdd),
    Search {
        who: Player,
        /// Whose library, and the other zones "and/or graveyard" adds.
        zones: Vec<Zone>,
        what: Objects,
        /// "for a Forest card and a Plains card" (Krosan Verge): more cards
        /// the one search finds, each by its own filter, where one filter
        /// in `what` would let a single card satisfy both.
        and: Vec<Objects>,
    },
    Scry(Amount),
    Surveil(Amount),
    Mill {
        who: Player,
        n: Amount,
    },
    /// A keyword action: proliferate, investigate, "it explores", "amass Zombies 2".
    Action {
        by: Option<Objects>,
        name: String,
        n: Option<Amount>,
    },
    Tap(Objects),
    Untap(Objects),
    Sacrifice {
        who: Player,
        what: Objects,
    },
    Fight {
        a: Objects,
        b: Objects,
    },
    /// "Those creatures fight each other": each of the objects deals damage
    /// equal to its power to the other (CR 701.14a).
    FightEachOther(Objects),
    Shuffle(Player),
    Reveal {
        who: Player,
        what: Objects,
    },
    LookAtTop {
        who: Player,
        whose: Player,
        n: Amount,
    },
    Restrict {
        what: Objects,
        rule: Restriction,
        dur: Duration,
    },
    GainControl {
        what: Objects,
        dur: Duration,
    },
    Transform(Objects),
    Regenerate(Objects),
    Attach {
        what: Objects,
        to: Objects,
    },
    Copy {
        what: Objects,
        may_choose_new_targets: bool,
    },
    /// "Take an extra turn after this one", "Target player takes two extra
    /// turns after this one", "After that turn, that player takes an extra
    /// turn" (Emrakul, the Promised End): `who` takes `n` extra turns
    /// directly after the turn `after` names. An effect adds them as it
    /// resolves, not through a delayed trigger (CR 500.7).
    ExtraTurn {
        who: Player,
        n: Amount,
        after: TurnRef,
    },
    RevealTop {
        who: Player,
        whose: Player,
        n: Amount,
    },
    /// "Create a token that's a copy of …", "create two tapped tokens that
    /// are copies of …", "create a tapped and attacking token that's a copy
    /// of …": `n` tokens, entering tapped, or attacking (CR 508.4: whom its
    /// controller chooses, unless `attacking` names it).
    CreateCopy {
        n: Amount,
        tapped: bool,
        attacking: Option<Recipient>,
        of: Objects,
        except: Vec<Exception>,
    },
    /// "turn a face-down creature you control face up" (Ugin's Mastery): its
    /// copiable values revert to its normal ones (CR 708.8).
    TurnFaceUp(Objects),
    ChooseNewTargets(Objects),
    ClassLevel(u32),
    May {
        who: Player,
        then: Vec<Effect>,
    },
    IfYouDont(Vec<Effect>),
    /// "Otherwise, …": the effects when the "if" before it did not hold,
    /// which "If you don't" (an option not taken) is not.
    Otherwise(Vec<Effect>),
    /// "that many plus one +1/+1 counters are put on it instead": said in
    /// the passive, so put by whoever was putting the counters a
    /// replacement watches for, not by "you".
    CountersArePut {
        n: Amount,
        kind: CounterKind,
        on: Objects,
    },
    Pay {
        who: Player,
        cost: Vec<Cost>,
    },
    During {
        dur: Duration,
        then: Vec<Effect>,
    },
    ExileUntilLeaves {
        what: Objects,
        until: Objects,
    },
    Prevent {
        combat_only: bool,
        n: Option<Amount>,
        to: Option<Vec<Recipient>>,
        by: Option<Objects>,
        dur: Duration,
    },
    Becomes {
        what: Objects,
        pt: Option<Pt>,
        types: Vec<Adj>,
        gains: Vec<Keyword>,
        keeps: Keeps,
        dur: Duration,
    },
    /// "this creature's base power and toughness become 4/2", "its base
    /// power becomes equal to the number of Towns you control": the base
    /// size set, layer 7b (CR 613.4b). `None` leaves that stat alone.
    BaseBecomes {
        what: Objects,
        power: Option<Amount>,
        toughness: Option<Amount>,
        dur: Duration,
    },
    BecomesCopy {
        what: Objects,
        of: Objects,
        dur: Duration,
        except: Vec<Exception>,
    },
    /// "you become the monarch", "you take the initiative".
    Designation(String),
    Delayed {
        step: Step,
        then: Vec<Effect>,
    },
    EntersTapped(Objects),
    Choose(Choice),
    GetEnergy(u32),
    Emblem(Vec<Ability>),
    /// "each player votes for death or taxes" (CR 701.38): each player chooses
    /// one of these words, which have no rules meaning (CR 701.38b). Said
    /// inside `Performed`, whose player votes.
    Vote(Vec<String>),
    /// "Simultaneously untap all tapped creatures and tap all untapped
    /// creatures", "that player simultaneously sacrifices the artifact and
    /// returns the artifact card to the battlefield" (Goblin Welder): the
    /// clauses happen at once, not one after the other ([simultaneously]).
    Simultaneously(Vec<Effect>),
    /// "Starting with you, each player chooses …": the players choose one at
    /// a time, `first` first and then in turn order, where CR 101.4 would
    /// start with the active player.
    StartingWith {
        first: Player,
        then: Vec<Effect>,
    },
    /// "Until end of turn, any time you could activate a mana ability, you
    /// may pay 1 life. If you do, add {C}." (Yavimaya Bloomsage), "Until end
    /// of turn, you may pay {1} any time you could cast an instant. If you
    /// do, …" (Guardian Angel): an action the player may take again and
    /// again in that window for the duration, each time paying the cost.
    AnyTime {
        who: Player,
        window: AnyTimeWindow,
        cost: Vec<Cost>,
        then: Vec<Effect>,
        dur: Duration,
    },
    /// "…, where X is …": X is defined for the whole sentence before it.
    Where {
        then: Vec<Effect>,
        x: Amount,
    },
    IfYouDo(Vec<Effect>),
    If {
        cond: Condition,
        then: Vec<Effect>,
    },
    /// "Until the end of your next turn, you may play those cards. If you
    /// cast a spell this way, mana of any type can be spent to cast it": a
    /// permission to cast, with the rules for each `spell` cast through it,
    /// applied as that spell is cast. They are part of the permission, not
    /// an "if" tested as the ability resolves, when nothing has been cast
    /// through it yet.
    PermissionRules {
        permission: Box<Effect>,
        spell: Objects,
        rules: Vec<Effect>,
    },
    Unless {
        effect: Box<Effect>,
        who: Player,
        cost: Vec<Cost>,
    },
    ForEach {
        each: Objects,
        then: Vec<Effect>,
    },
    /// "For each color among permanents you control, add one mana of that
    /// color": the effect once per color, with "that color" bound to it.
    ForEachColor {
        among: Palette,
        then: Vec<Effect>,
    },
    Modal {
        choose: Choose,
        modes: Vec<Vec<Effect>>,
    },
    /// "Creatures you control get +1/+1 until end of turn" style abilities that
    /// a spell grants for a while.
    GrantAbility {
        what: Objects,
        ability: Box<Ability>,
        dur: Duration,
    },
    LoseAbility {
        what: Objects,
        ability: Box<Ability>,
        dur: Duration,
    },
    /// "The owner of target permanent shuffles it into their library".
    ShuffleInto {
        who: Player,
        what: Objects,
    },
    /// An effect whose verb takes no player, done by one: "they put it onto
    /// the battlefield", "its controller adds an additional {G}".
    Performed {
        by: Player,
        effect: Box<Effect>,
    },
    /// "exile target player's graveyard", "exile all graveyards".
    ExileZone(Zone),
    /// "You may play that card this turn", "cast it without paying its mana cost".
    Cast {
        verb: CastVerb,
        what: Objects,
        payment: Payment,
        dur: Duration,
    },
    /// "you may cast it by paying {1} rather than paying its mana cost", "cast
    /// target instant or sorcery card from your graveyard by paying {R}{R} in
    /// addition to its other costs": an alternative cost applied by this
    /// effect (`rather`, CR 118.9), or an additional cost (CR 118.8).
    CastPaying {
        what: Objects,
        cost: Vec<Cost>,
        rather: bool,
    },
    /// "you gain protection from everything".
    PlayerGains {
        who: Player,
        keywords: Vec<Keyword>,
        dur: Duration,
    },
    PlayerRestrict {
        who: Player,
        rule: PlayerRule,
        dur: Duration,
    },
    PhaseOut(Objects),
    /// "If you control six or more lands, create … instead": the effect
    /// before it, replaced when the condition holds.
    Instead {
        cond: Condition,
        then: Vec<Effect>,
    },
    /// "create a Food token or a Treasure token": one of them, by choice.
    Or(Vec<Effect>),
    /// "it creates twice that many of those tokens": more of tokens an event
    /// just described, not copies of permanents.
    CreateThose {
        by: Actor,
        n: Amount,
        those: Objects,
    },
    PutThose {
        by: Actor,
        n: Amount,
        those: Objects,
        on: Objects,
    },
    /// "When this Aura leaves the battlefield, …" inside an effect: a delayed
    /// triggered ability (CR 603.7).
    DelayedWhen {
        event: Event,
        then: Vec<Effect>,
    },
    /// "When you next cast an instant or sorcery spell this turn, copy that
    /// spell": a delayed trigger that triggers once, the next time, and only
    /// within `dur` (CR 603.7b); "this turn" alone would let it trigger
    /// every time.
    DelayedNext {
        event: Event,
        dur: Duration,
        then: Vec<Effect>,
    },
    /// "When you do, …": a reflexive triggered ability (CR 603.12), which goes
    /// on the stack, unlike "If you do".
    WhenYouDo(Vec<Effect>),
    /// "When you do, if you control a red permanent other than Ajani, …": a
    /// reflexive trigger with an intervening "if" (CR 603.12, 603.4), which
    /// triggers only if it holds and checks it again as it resolves.
    WhenYouDoIf {
        cond: Condition,
        then: Vec<Effect>,
    },
    /// A modal spell's mode with a flavour label: "Sell Contraband — …".
    Labelled {
        label: String,
        then: Vec<Effect>,
    },
    /// "Change the target of target spell or ability with a single target."
    ChangeTarget(Objects),
    /// "change the target of target spell with a single target to this creature".
    ChangeTargetTo {
        what: Objects,
        to: Objects,
    },
    /// "Exile the top card of your library face down."
    ExileFaceDown(Objects),
    /// "exile the top card of your library face down and look at it", "You
    /// may look at that card for as long as it remains exiled": a look at
    /// objects already named, once or (with a duration) any time while it
    /// lasts.
    LookAt {
        who: Player,
        what: Objects,
        dur: Duration,
    },
    /// "Exchange your life total with this creature's toughness": the
    /// player's life total becomes the stat and the stat the life total
    /// (CR 701.12a: all of it or nothing).
    ExchangeLifeWithStat {
        who: Player,
        what: Objects,
        stat: Stat,
    },
    /// "Exchange control of two target creatures", "exchange control of this
    /// creature and target creature": each controller gains control of the
    /// permanent the other controlled, and nothing happens unless the whole
    /// exchange can be made (CR 701.12a-b). One object for "two target …",
    /// two for "X and Y". It lasts indefinitely.
    ExchangeControl(Vec<Objects>),
    /// "exchange life totals with target opponent": each gains or loses
    /// the life to equal the other's previous total (CR 701.12c).
    ExchangeLifeTotals {
        who: Player,
        with: Player,
    },
    /// "Look at target player's hand."
    LookAtHand {
        who: Player,
        whose: Player,
    },
    /// "You may cast spells this turn as though they had flash."
    CastAsThoughFlash {
        what: Objects,
        dur: Duration,
    },
    /// "Permanents your opponents control lose hexproof and indestructible".
    LoseKeywords {
        what: Objects,
        keywords: Vec<Keyword>,
        dur: Duration,
    },
    LoseGame(Player),
    /// "You may play an additional land this turn".
    ExtraLandPlays {
        n: Amount,
        dur: Duration,
    },
    /// "amass Orcs 1".
    Amass {
        of: String,
        n: Amount,
    },
    /// "exile it instead of putting it into its owner's graveyard".
    InsteadOf {
        then: Box<Effect>,
        of: Box<Effect>,
    },
    /// A keyword action a player does to objects: "manifests the top card of
    /// their library".
    ActionOn {
        who: Player,
        name: String,
        what: Objects,
    },
    /// "move all counters from ~ onto target creature".
    MoveCounters {
        counters: Objects,
        from: Objects,
        to: Objects,
    },
    /// "put those counters on ~": counters an earlier event named, of the
    /// same kinds and number.
    PutThoseCounters {
        those: Objects,
        on: Objects,
    },
    /// "exile it with a void counter on it".
    ExileWithCounters {
        what: Objects,
        n: Amount,
        kind: CounterKind,
    },
    /// "double the power and toughness of each creature you control".
    Double {
        stats: Vec<Stat>,
        of: Objects,
        dur: Duration,
    },
    WinGame(Player),
    /// "roll a d20".
    RollDie(u32),
    /// "roll X six-sided dice", "roll two six-sided dice": `n` dice of
    /// `sides` sides, rolled together (CR 706.1).
    RollDice {
        n: Amount,
        sides: u32,
    },
    /// "you don't lose this mana as steps and phases end".
    KeepsMana {
        who: Player,
        mana: KeptMana,
    },
    /// "Repeat the following process X times. …", "For each 1 damage
    /// prevented this way, …": the clauses, that many times over.
    Repeat {
        times: Amount,
        then: Vec<Effect>,
    },
    /// "Repeat this process until you put a card into your hand or …": the
    /// enclosing `Process` again, until the event happens.
    RepeatUntil(Box<Event>),
    /// "this process": the instructions a "repeat this process" inside them
    /// starts again from the first of. Every one in the corpus is the whole
    /// ability: Cultivator Colossus repeats putting a land too, not only
    /// drawing (its ruling: "until you decline to put a land card").
    Process(Vec<Effect>),
    /// "shuffle your hand and graveyard into your library": every card in
    /// those zones.
    ShuffleZones {
        who: Player,
        zones: Vec<Zone>,
    },
    /// "You have no maximum hand size for the rest of the game."
    NoMaxHandSize {
        who: Player,
        dur: Duration,
    },
    /// "it enters with a number of additional +1/+1 counters on it equal to …".
    EntersWithCounters {
        what: Objects,
        n: Amount,
        kind: CounterKind,
    },
    /// "Distribute two +1/+1 counters among one or two target creatures".
    DistributeCounters {
        n: Amount,
        kind: CounterKind,
        among: Objects,
    },
    /// "After this main phase, there is an additional combat phase followed
    /// by an additional main phase." `after` is `None` for "this phase".
    AdditionalPhases {
        after: Option<PhaseKind>,
        phases: Vec<PhaseKind>,
    },
    /// "Each opponent attacking that player does the same": the effects
    /// before it, done by these players.
    DoesTheSame(Player),
    /// "An opponent separates those cards into two piles."
    SeparatePiles {
        who: Player,
        what: Objects,
        n: u32,
    },
    /// "If it would leave the battlefield, exile it instead …" said by a
    /// one-shot effect: a replacement effect it creates.
    Replacement {
        event: Box<Event>,
        instead: Vec<Effect>,
    },
    /// "double the number of +1/+1 counters on this creature".
    DoubleCounters {
        kind: Option<CounterKind>,
        on: Objects,
    },
    /// "it deals double that damage instead": the damage of the event a
    /// replacement watches for, with a new amount, to its recipient unless
    /// named again.
    ThatDamage {
        source: Objects,
        n: Amount,
        to: Option<Vec<Recipient>>,
    },
    /// "each player exiles cards from the top of their library until they
    /// exile a nonland card".
    UntilFound {
        who: Player,
        verb: DigVerb,
        zone: Zone,
        until: Objects,
    },
    /// "they get that many poison counters".
    GetCounters {
        who: Player,
        n: Amount,
        kind: CounterKind,
    },
    /// "Until end of turn, whenever a player taps an Island for mana, …": a
    /// delayed triggered ability that triggers each time, where `DelayedWhen`
    /// triggers once.
    DelayedWhenever {
        event: Event,
        then: Vec<Effect>,
    },
    /// "those tokens plus an additional Food token are created instead".
    CreateThosePlus {
        by: Actor,
        those: Objects,
        n: Amount,
        token: Token,
    },
    /// "Remove target attacking creature you control from combat".
    RemoveFromCombat(Objects),
    /// "that player skips that turn": the turn an event named.
    SkipTurn(Player),
    /// "creatures you control … gain all creature types": types added to
    /// the ones they have.
    AlsoIs {
        what: Objects,
        types: TypeSet,
        dur: Duration,
    },
    /// "you may spend mana as though it were mana of any color to cast them".
    /// `dur` is how long the permission lasts: the text never says it, and
    /// it is the play permission's it is given with ("You may play those
    /// cards for as long as they remain exiled, and you may spend mana …"),
    /// since mana is spent to cast them only while they may be cast.
    /// `Permanent` where no play permission is said with it.
    SpendAsAnyColor {
        uses: Vec<SpendUse>,
        dur: Duration,
    },
    /// "… as though it were mana of any type": colorless too, one of the six
    /// types (CR 106.1b), so colored mana pays a {C} symbol, which "any
    /// color" does not allow. `dur` as for `SpendAsAnyColor`.
    SpendAsAnyType {
        uses: Vec<SpendUse>,
        dur: Duration,
    },
    /// "It's an enchantment.": what a permanent an effect returns is as it
    /// enters, where "becomes" would apply only after (CR 611.2e).
    IsNow {
        what: Objects,
        types: Vec<Adj>,
        keeps: Keeps,
        /// "Each of them is a 1/1 Spirit": the size it enters with, its base
        /// power and toughness (CR 613.4b).
        pt: Option<Pt>,
        /// "… with flying": keywords it has as it enters.
        gains: Vec<Keyword>,
    },
    /// "return it to the battlefield with a -1/-1 counter on it": the moved
    /// permanent is given them as it enters (CR 122.6), not after.
    EntersWith {
        effect: Box<Effect>,
        n: Amount,
        kind: CounterKind,
    },
    /// "return this card from your graveyard to the battlefield tapped and
    /// attacking": the moved creature enters attacking, what it attacks
    /// its controller's choice (CR 508.4); it never "attacked".
    EntersAttacking(Box<Effect>),
    /// "return target Aura or Equipment card from your graveyard to the
    /// battlefield attached to a creature you control": the moved Aura or
    /// Equipment enters attached to `to`, which the effect names, so no
    /// player chooses it (CR 303.4f).
    EntersAttached {
        effect: Box<Effect>,
        to: Objects,
    },
    /// "Damage can't be prevented this turn."
    DamageCantBePrevented(Duration),
    /// "Target player reveals their hand": the player's own hand.
    RevealHand(Player),
    /// "up to one other target creature loses all abilities".
    Loses {
        what: Objects,
        lost: Vec<Lost>,
        dur: Duration,
    },
    /// "gains your choice of double strike or lifelink": one of these, picked
    /// as it resolves.
    GainsChoiceOf {
        what: Objects,
        keywords: Vec<Keyword>,
        dur: Duration,
    },
    /// "Shuffle this artifact and your graveyard into their owner's library":
    /// objects and whole zones, one shuffle.
    ShuffleAllInto {
        who: Player,
        what: Vec<Objects>,
        zones: Vec<Zone>,
    },
    /// "You get X {E}".
    GetEnergyAmount(Amount),
    /// "draw a card and repeat this process": the enclosing `Process` again.
    RepeatProcess,
    /// "you gain control of target opponent during that player's next turn":
    /// always the controlled player's next turn (CR 723.1).
    ControlPlayerNextTurn(Player),
    /// "becomes an artifact creature with power and toughness each equal to
    /// its mana value".
    BecomesSized {
        what: Objects,
        types: Vec<Adj>,
        /// What it keeps of its types: Karn's "becomes an artifact creature"
        /// keeps all of them (CR 205.1b).
        keeps: Keeps,
        each_equal_to: Amount,
        dur: Duration,
    },
    /// "The flashback cost is equal to its mana cost.": the cost of a
    /// keyword the effect before it granted.
    KeywordCostIs {
        keyword: String,
        cost: Vec<Cost>,
    },
    /// "create a colorless artifact token named Munitions with …".
    CreateNamed {
        who: Player,
        n: Amount,
        token: Token,
        name: String,
    },
    /// "Create a Monster Role token attached to it", "create a white Aura
    /// enchantment token named Contract attached to target creature an
    /// opponent controls" (Scriv): the name, where one is given.
    CreateAttached {
        who: Player,
        n: Amount,
        token: Token,
        to: Objects,
        name: Option<String>,
    },
    /// "Each opponent gets an emblem with …".
    EmblemFor {
        who: Player,
        abilities: Vec<Ability>,
    },
    /// "For each card type, …": once per card type (CR 205.2a), with "that
    /// type" bound to it.
    ForEachCardType(Vec<Effect>),
    FlipCoin,
    /// "instant and sorcery spells you cast cost {1} less to cast" for a while.
    CostChange {
        what: Objects,
        by: Mana,
        less: bool,
        dur: Duration,
    },
    /// "deals 2 damage divided as you choose among one or two targets".
    DamageDivided {
        source: Option<Objects>,
        n: Amount,
        among: Objects,
    },
    /// "end the turn" (CR 724.1).
    EndTheTurn,
    /// "exiles cards from the top of their library until that player has
    /// exiled cards with total mana value 20 or greater".
    DigUntilTotal {
        who: Player,
        verb: DigVerb,
        zone: Zone,
        total: Compare,
    },
    /// "put two +1/+1 counters and a flying counter on target creature".
    PutCountersEach {
        counters: Vec<(Amount, CounterKind)>,
        on: Objects,
    },
    /// "When a creature card is exiled this way, …": a reflexive triggered
    /// ability on what the ability's own words did (CR 603.12).
    WhenThisWay {
        event: Event,
        then: Vec<Effect>,
    },
    RemoveAllCounters(Objects),
    /// "remove all fate counters from all permanents": every counter of one
    /// kind.
    RemoveAllCountersOf {
        kind: CounterKind,
        from: Objects,
    },
    /// "Target opponent loses all counters."
    LosesAllCounters(Player),
    /// "The next time a source of your choice would deal damage to you this
    /// turn, …": a one-shot effect on the next such event (CR 609.7, 615.8).
    NextTime {
        event: Box<Event>,
        dur: Duration,
        then: Vec<Effect>,
    },
    /// "prevent that damage": the damage of the event the replacement watches.
    PreventThatDamage,
    /// "meld them into Hanweir, the Writhing Township" (CR 701.42).
    Meld {
        what: Objects,
        into: String,
        /// The melded card's name begins with this card's short name, which
        /// the text says as ~ ("meld them into ~, Gaea Incarnate").
        shares_short_name: bool,
    },
    /// "it produces {C} instead of any other type and amount".
    ProducesInstead {
        what: Objects,
        mana: Mana,
    },
    /// "Add {R}{R}. If any of that mana is spent on a creature spell, it
    /// gains haste until end of turn.": an effect of the mana itself on the
    /// spell it pays for, applied when it is spent (CR 106.6), not a check
    /// as the ability resolves, when none of it has been spent yet.
    OnSpend {
        of: SpentOf,
        on: Objects,
        then: Vec<Effect>,
    },
    /// "this creature becomes prepared": while it is, its controller may
    /// cast a copy of its inset spell (CR 722.3).
    /// "double the value of X" (Unbound Flourishing): the X chosen for that
    /// spell becomes twice as much, after it was cast.
    DoubleX(Objects),
    BecomesPrepared(Objects),
    /// "it becomes plotted": a card in exile its owner may cast without
    /// paying its mana cost as a sorcery on a later turn (CR 702.170c-d).
    BecomesPlotted(Objects),
    /// "target Mount you control becomes saddled until end of turn": a
    /// designation that lasts the turn (CR 702.171b).
    BecomesSaddled(Objects),
    /// "remove up to three counters from target creature": of any kinds,
    /// as the player picks.
    RemoveAnyCounters {
        n: Amount,
        from: Objects,
    },
}

/// What mana was spent on, in "mana from a Treasure was spent to …".
#[derive(Clone, Debug, PartialEq)]
pub enum SpentFor {
    /// "to cast it", "to cast this spell".
    Cast(Objects),
    /// "to activate this ability".
    ThisAbility,
    /// "to cast it or activate it": its casting, or an ability of it.
    CastOrActivate(Objects),
}

/// How much of the mana an ability made a spell must be paid with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpentOf {
    /// "If any of that mana is spent on …": some of it.
    Any,
    /// "If you spend this mana to cast …": the mana, all of it.
    All,
}

impl Keeps {
    /// What a type change keeps when it says nothing about it, by the types
    /// it sets. Subtypes alone replace only their own set (CR 205.1a);
    /// "becomes an artifact creature" keeps every prior card type and
    /// subtype, and "a Construct artifact creature" all but the creature
    /// types (CR 205.1b); any other card type replaces the card types.
    pub fn implied_by(types: &[Adj]) -> Keeps {
        // "becomes a blue Serpent": a color is no type (CR 205.1a replaces
        // only the types an effect sets; CR 105.3 sets the color).
        let types: Vec<Adj> = types
            .iter()
            .filter(|a| !matches!(a, Adj::Color(_)))
            .cloned()
            .collect();
        let types = &types[..];
        // "has base power and toughness 4/4", "becomes black", "becomes
        // blocked": an effect that sets no type replaces none. "becomes
        // snow": a supertype gained keeps every other type (CR 205.4b).
        let types: Vec<Adj> = types
            .iter()
            .filter(|a| !matches!(a, Adj::State(_)))
            .cloned()
            .collect();
        let types = &types[..];
        if types.iter().all(|a| matches!(a, Adj::Supertype(_))) {
            return Keeps::OtherTypes;
        }
        let card_types: Vec<&str> = types
            .iter()
            .filter_map(|a| match a {
                Adj::Type(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        let subtypes = types.iter().any(|a| matches!(a, Adj::Subtype(_)));
        let only_subtypes = !types.is_empty() && types.iter().all(|a| matches!(a, Adj::Subtype(_)));
        match (&card_types[..], subtypes) {
            _ if only_subtypes => Keeps::AllButSubtypesOfSet,
            (["artifact", "creature"], false) => Keeps::OtherTypes,
            (["artifact", "creature"], true) => Keeps::AllButSubtypesOfSet,
            _ => Keeps::Supertypes,
        }
    }
}

impl Effect {
    /// The copy a following "You may choose new targets for the copy" is
    /// about: the `Copy` this effect ends with, through the branches,
    /// loops and delayed triggers that hold it ("copy it for each time
    /// you've cast your commander …", "When you next cast an instant or
    /// sorcery spell this turn, copy it"), but not through a "may", whose
    /// copy says its own "and may choose a new target for the copy".
    /// Returns its `may_choose_new_targets`.
    pub fn trailing_copy_mut(&mut self) -> Option<&mut bool> {
        match self {
            Effect::Copy {
                may_choose_new_targets,
                ..
            } => Some(may_choose_new_targets),
            Effect::Repeat { then, .. }
            | Effect::DelayedNext { then, .. }
            | Effect::DelayedWhen { then, .. }
            | Effect::DelayedWhenever { then, .. }
            | Effect::During { then, .. }
            | Effect::IfYouDont(then)
            | Effect::If { then, .. }
            | Effect::Instead { then, .. }
            | Effect::Where { then, .. }
            | Effect::IfYouDo(then)
            | Effect::ForEach { then, .. }
            | Effect::May { then, .. }
            | Effect::WhenYouDo(then) => then.last_mut()?.trailing_copy_mut(),
            _ => None,
        }
    }

    /// "Copy target instant or sorcery spell, then return it to its owner's
    /// hand. You may choose new targets for the copy." (Narset's Reversal):
    /// a move of the copied spell itself, said between the copy and its
    /// new-targets sentence, which is still about the copy ([copy-new-targets]).
    pub fn moves_the_original(&self) -> bool {
        matches!(self, Effect::Move { what, .. }
            if what.quant == Quant::Antecedent && what.filter == Filter::default())
    }

    pub fn trailing_copy(&self) -> Option<bool> {
        match self {
            Effect::Copy {
                may_choose_new_targets,
                ..
            } => Some(*may_choose_new_targets),
            Effect::Repeat { then, .. }
            | Effect::DelayedNext { then, .. }
            | Effect::DelayedWhen { then, .. }
            | Effect::DelayedWhenever { then, .. }
            | Effect::During { then, .. }
            | Effect::IfYouDont(then)
            | Effect::If { then, .. }
            | Effect::Instead { then, .. }
            | Effect::Where { then, .. }
            | Effect::IfYouDo(then)
            | Effect::ForEach { then, .. }
            | Effect::May { then, .. }
            | Effect::WhenYouDo(then) => then.last()?.trailing_copy(),
            _ => None,
        }
    }

    /// The one object an effect is said of, where it names no other:
    /// "~ gains indestructible until end of turn", "Tap it". A pronoun in
    /// the next sentence can only point back to it.
    pub fn sole_object(&self) -> Option<Objects> {
        self.clone().sole_object_mut().cloned()
    }

    pub fn sole_object_mut(&mut self) -> Option<&mut Objects> {
        match self {
            Effect::Modify { what, .. }
            | Effect::GrantAbility { what, .. }
            | Effect::Becomes { what, .. }
            | Effect::Tap(what)
            | Effect::Untap(what)
            | Effect::Transform(what)
            | Effect::PhaseOut(what) => Some(what),
            Effect::PutCounters { on, .. } => Some(on),
            _ => None,
        }
    }
}

impl Effect {
    /// The effect's own duration, for an effect that lasts: "gets +1/+1
    /// until end of turn", "gains "…" until end of turn".
    pub fn dur_mut(&mut self) -> Option<&mut Duration> {
        match self {
            Effect::AlsoIs { dur, .. }
            | Effect::BaseBecomes { dur, .. }
            | Effect::Becomes { dur, .. }
            | Effect::BecomesCopy { dur, .. }
            | Effect::BecomesSized { dur, .. }
            | Effect::Cast { dur, .. }
            | Effect::CastAsThoughFlash { dur, .. }
            | Effect::CostChange { dur, .. }
            | Effect::ExtraLandPlays { dur, .. }
            | Effect::GainControl { dur, .. }
            | Effect::GainsChoiceOf { dur, .. }
            | Effect::GrantAbility { dur, .. }
            | Effect::LookAt { dur, .. }
            | Effect::LoseAbility { dur, .. }
            | Effect::LoseKeywords { dur, .. }
            | Effect::Loses { dur, .. }
            | Effect::Modify { dur, .. }
            | Effect::NoMaxHandSize { dur, .. }
            | Effect::PlayerGains { dur, .. }
            | Effect::PlayerRestrict { dur, .. }
            | Effect::Prevent { dur, .. }
            | Effect::Restrict { dur, .. }
            | Effect::SpendAsAnyColor { dur, .. }
            | Effect::SpendAsAnyType { dur, .. } => Some(dur),
            _ => None,
        }
    }
}

/// The one targeted object of a clause a trailing step delays, as
/// [target-before-delayed] chooses it before the delayed trigger.
pub fn delayed_target_mut(e: &mut Effect) -> Option<&mut Objects> {
    let x = match e {
        Effect::Destroy { what, .. } => what,
        e => e.sole_object_mut()?,
    };
    matches!(
        x.quant,
        Quant::Target { .. } | Quant::OtherTarget { .. } | Quant::AnotherTarget
    )
    .then_some(x)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseKind {
    Combat,
    Main,
}

/// How a player goes through a library one card at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigVerb {
    Exile,
    Reveal,
}

/// What "loses" takes away in a static: keywords, or every ability or type
/// of a kind, perhaps but those the same effect grants.
#[derive(Clone, Debug, PartialEq)]
pub enum Lost {
    Keywords(Vec<Keyword>),
    /// "all abilities"; with `other`, "all other abilities".
    All {
        other: bool,
        of: Characteristic,
    },
}

/// A value one object takes from another in `Static::HasValuesOf`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopiedValue {
    Power,
    Toughness,
    CreatureTypes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Characteristic {
    Abilities,
    CardTypes,
    CreatureTypes,
    /// "loses all land types" (Ultima, Origin of Oblivion): every land subtype (CR 205.3i).
    LandTypes,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeptMana {
    /// "this mana": what the ability just added.
    This,
    /// "unspent red mana", or any color when `None`.
    Unspent(Option<Color>),
}

/// What a type-changing effect leaves of the types before it (CR 205.1b).
#[derive(Clone, Debug, PartialEq)]
pub enum Keeps {
    /// "becomes a 4/4 red Dragon creature": the card types and subtypes it
    /// sets replace the old (CR 205.1a), and its supertypes stay, which
    /// changing card types or subtypes never changes (CR 205.4b): a
    /// legendary planeswalker is a legendary Dragon.
    Supertypes,
    /// "becomes an Island", "becomes a Coward": only subtypes are set, so
    /// only the subtypes of their set are replaced; card types, supertypes
    /// and other subtypes stay (CR 205.1a, 305.7). Tide Shaper's land is
    /// still a land, and still basic or legendary. Also "becomes a 2/2
    /// Construct artifact creature": it keeps every prior card type and
    /// subtype but replaces its creature types (CR 205.1b, last sentence),
    /// where a bare "artifact creature" keeps everything (`OtherTypes`).
    AllButSubtypesOfSet,
    /// "in addition to its other types".
    OtherTypes,
    /// "in addition to its other colors and types": the colors it sets are
    /// added too, where a color an effect gives otherwise replaces the old
    /// (CR 105.3).
    OtherColorsAndTypes,
    /// "in addition to its other creature types": only creature types are
    /// added, and every type it had stays (CR 205.1b). A color said with it
    /// replaces the old (CR 105.3): Chainer's "That creature is black and
    /// is a Nightmare in addition to its other creature types".
    OtherCreatureTypes,
    /// "that's still a land".
    Listed(Vec<Adj>),
}

/// A copy effect's "except …": values the copy has that the original does
/// not (CR 707.9).
#[derive(Clone, Debug, PartialEq)]
pub enum Exception {
    /// "it's an artifact in addition to its other types".
    AlsoIs(Vec<Adj>),
    /// "it isn't legendary", "the token isn't legendary".
    IsNot(Vec<Adj>),
    /// "it has haste", "it has haste and "At the beginning …"": keywords,
    /// then quoted abilities, as Oracle text lists them.
    Has {
        keywords: Vec<Keyword>,
        abilities: Vec<Ability>,
    },
    /// "except it has this ability": the copy keeps the ability that made it.
    HasThisAbility,
    /// "it enters with an additional +1/+1 counter on it if it's a creature".
    EntersWithCounters {
        n: Amount,
        kind: CounterKind,
        cond: Option<Box<Condition>>,
    },
    /// "his name is ~": the copy keeps this card's name.
    NameIsSelf,
    /// "he's a 4/4 Spider Human Hero in addition to his other types": a size
    /// and types on top of the copied ones.
    AlsoIsSized { pt: Pt, types: Vec<Adj> },
    /// "except it's legendary", "except it's a 4/4 black Zombie", "except
    /// it's 1/1": values the copy has in place of the copied ones, part of
    /// its copiable values (CR 707.9b).
    Is { pt: Option<Pt>, adjs: Vec<Adj> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Any,
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastVerb {
    Play,
    Cast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payment {
    ManaCost,
    /// "without paying its mana cost".
    Free,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerRule {
    LifeTotalCantChange,
    CantCast(Objects),
    CantActivateAbilitiesOf(Objects),
    /// "can't draw more than one card each turn".
    CantDrawMoreThanEachTurn(u32),
    /// "can't cast more than one noncreature spell each turn".
    CantCastMoreThanEachTurn {
        n: u32,
        what: Filter,
    },
    CantGainLife,
    CantSearchLibraries,
    /// "That player can't get counters".
    CantGetCounters,
    /// "they can't attack you or planeswalkers you control": the player
    /// declares no attack on any of these (CR 508.1a).
    CantAttack(Vec<Recipient>),
}

/// Who does a thing a replacement effect watches for.
#[derive(Clone, Debug, PartialEq)]
pub enum Actor {
    Player(Player),
    /// "If an effect would create …".
    AnEffect,
    /// "it creates …", the effect just named.
    ThatEffect,
    /// "would be created", "are created instead": the text names no one.
    Passive,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Recipient {
    Objects(Objects),
    Player(Player),
    /// "target player or planeswalker": one recipient of either kind.
    Or(Vec<Recipient>),
    /// "the player or planeswalker it's attacking".
    AttackedBy(Objects),
    /// "~ deals 4 damage instead": the recipient of the effect this one
    /// replaces, named there and not again.
    Same,
    /// "tokens that are tapped and attacking": attacking whom its controller
    /// chooses as it enters (CR 508.4).
    ControllerChooses,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Choose {
    One,
    N(u32),
    OneOrBoth,
    OneOrMore,
    AnyNumber,
    UpTo(u32),
    /// "Choose one. If you control a commander as you cast this spell, you
    /// may choose both instead." `may` is false for "If this spell was cast
    /// using teamwork, choose both instead.", where the caster has no choice.
    Upgrade {
        base: Box<Choose>,
        cond: Box<Condition>,
        to: Box<Choose>,
        may: bool,
    },
    /// "choose one that hasn't been chosen this turn".
    NotChosenThisTurn(Box<Choose>),
    /// "Choose three. You may choose the same mode more than once." (CR
    /// 700.2d: a mode chosen twice is done twice, in sequence).
    Repeatable(Box<Choose>),
    /// "Choose one or both. Each mode must target a different player."
    DifferentPlayers(Box<Choose>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ManaAdd {
    Symbols(Mana),
    Or(Vec<Mana>),
    /// "one mana of any color in your commander's color identity".
    Of {
        n: Amount,
        pick: Pick,
        within: Option<Palette>,
    },
    AmountOf {
        mana: Mana,
        n: Amount,
    },
    /// "adds an additional {G}": on top of the mana that triggered it.
    Additional(Box<ManaAdd>),
}

/// How the colors of added mana are picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    AnyColor,
    AnyOneColor,
    AnyCombination,
    /// "any type": colorless too.
    AnyType,
    /// "mana of the chosen color": no pick, the color is given.
    Stated,
    /// "three mana of that type": the type of mana named before, colorless too.
    ThatType,
}

/// The players a loop over "each player" / "each opponent" runs through.
fn player_loop_objects(who: &Player) -> Option<Objects> {
    let kind = match who {
        Player::Each => Noun::Player,
        Player::EachOpponent => Noun::Opponent,
        _ => return None,
    };
    Some(Objects {
        quant: Quant::Plural,
        filter: Filter {
            kind: Some(kind),
            ..Filter::default()
        },
    })
}

/// "Each player creates a 0/0 … Fractal creature token and puts a number of
/// +1/+1 counters on it equal to …" (Oversimplify): each player puts the
/// counters on the token that player created, so the two clauses are one
/// pass per player, `ForEach { players, [Create { who: That }, Performed {
/// by: That, .. }] }`. Read as two loops, `Created` was every player's
/// token, and each total went on all of them ([each-player-creates]).
pub fn each_player_creates(es: &mut Vec<Effect>) {
    struct HasCreated(bool);
    impl crate::walk::Visitor for HasCreated {
        fn enter(&mut self, o: &mut Objects) -> bool {
            self.0 |= o.quant == Quant::Created;
            true
        }
    }
    let mut i = 0;
    while i + 1 < es.len() {
        let pair = match (&es[i], &es[i + 1]) {
            (Effect::Create { who, .. }, Effect::Performed { by, effect }) if who == by => {
                let mut e = (**effect).clone();
                let mut has = HasCreated(false);
                crate::walk::Walk::walk(&mut e, &mut has);
                has.0
            }
            _ => false,
        };
        let each = match &es[i] {
            Effect::Create { who, .. } if pair => player_loop_objects(who),
            _ => None,
        };
        if let Some(each) = each {
            let mut then: Vec<Effect> = es.drain(i..i + 2).collect();
            if let Effect::Create { who, .. } = &mut then[0] {
                *who = Player::That;
            }
            if let Effect::Performed { by, .. } = &mut then[1] {
                *by = Player::That;
            }
            es.insert(i, Effect::ForEach { each, then });
        }
        i += 1;
    }
}

/// "Starting with you, each player chooses up to one permanent …. Exile
/// those permanents." (The Legend of Yangchen), "Each opponent chooses a
/// creature card in their graveyard. Put those cards onto the battlefield
/// under your control." (Dredge the Mire): a reference back in the next
/// sentence is every object the players chose, `Each` with
/// `Marked(Chosen)`, as "Destroy each permanent chosen this way" says it. A
/// bare `Antecedent` was the last player's choice alone
/// ([each-player-chose]).
pub fn each_player_chose(es: &mut [Effect]) {
    struct Those(bool);
    impl crate::walk::Visitor for Those {
        fn enter(&mut self, o: &mut Objects) -> bool {
            if self.0 {
                return false;
            }
            self.0 = true;
            if o.quant == Quant::Antecedent && o.filter.posts.is_empty() && o.filter.or.is_none() {
                o.quant = Quant::Each;
                o.filter.posts.push(Post::Marked(Marked::Chosen));
            }
            false
        }
    }
    fn chose(e: &Effect) -> bool {
        match e {
            Effect::StartingWith { then, .. } => matches!(&then[..], [x] if chose(x)),
            Effect::Performed {
                by: Player::Each | Player::EachOpponent,
                effect,
            } => matches!(**effect, Effect::Choose(Choice::Objects(_))),
            _ => false,
        }
    }
    for e in es.iter_mut() {
        if let Effect::Labelled { then, .. } = e {
            each_player_chose(then);
        }
    }
    for i in 1..es.len() {
        // "and sacrifices it" continues the chooser's own clause.
        let continued = matches!(
            es[i],
            Effect::Performed { .. }
                | Effect::Sacrifice {
                    who: Player::Each | Player::EachOpponent,
                    ..
                }
        );
        if chose(&es[i - 1]) && !continued {
            crate::walk::Walk::walk(&mut es[i], &mut Those(false));
        }
    }
}

/// "Choose target creature you control and target creature you don't
/// control. … Then those creatures fight each other": the fight is both
/// targets, `Listed` ([choose-targets], [fight-each-other]). `false` when a
/// later clause says "each of those creatures deals damage … to the
/// other" (Grim Contest), which no node holds: one source and `Rest` would
/// be one creature's damage.
pub fn choose_targets_listed(es: &mut [Effect]) -> bool {
    let Some(i) = es
        .iter()
        .position(|e| matches!(e, Effect::Choose(Choice::Targets(_))))
    else {
        return true;
    };
    for e in es[i + 1..].iter_mut() {
        match e {
            Effect::FightEachOther(o) if o.quant == Quant::Antecedent => o.quant = Quant::Listed,
            Effect::Damage { to, .. }
                if to
                    .iter()
                    .any(|t| matches!(t, Recipient::Objects(o) if o.quant == Quant::Rest)) =>
            {
                return false
            }
            _ => {}
        }
    }
    true
}

/// The renderer's inverse of `each_player_creates`: the pass per player is
/// said "each player creates … and puts …", as the card says it.
pub fn each_player_creates_said(es: &mut Vec<Effect>) {
    let mut i = 0;
    while i < es.len() {
        let who = match &es[i] {
            Effect::ForEach { each, then } => match &then[..] {
                [Effect::Create {
                    who: Player::That, ..
                }, Effect::Performed {
                    by: Player::That, ..
                }] => [Player::Each, Player::EachOpponent]
                    .into_iter()
                    .find(|p| player_loop_objects(p).as_ref() == Some(each)),
                _ => None,
            },
            _ => None,
        };
        if let Some(who) = who {
            let Effect::ForEach { mut then, .. } = es.remove(i) else {
                unreachable!()
            };
            if let Effect::Create { who: w, .. } = &mut then[0] {
                *w = who.clone();
            }
            if let Effect::Performed { by, .. } = &mut then[1] {
                *by = who;
            }
            es.splice(i..i, then);
            i += 2;
        } else {
            i += 1;
        }
    }
}

/// "If you would create a Clue, Food, or Treasure token, instead create one
/// of each" (Academy Manufactor): that many of each predefined token the
/// replaced event names (CR 111.10), each a `Create` of the same token "create
/// a Treasure token" reads into, and never the replaced token itself (a Food
/// copy of Carrot Cake still makes a plain Food). The Scryfall ruling says
/// it: "you will instead create that many Clue tokens, that many Food
/// tokens, and that many Treasure tokens" ([one-of-each]).
pub fn one_of_each(event: &Event, who: &Player) -> Option<Vec<Effect>> {
    let Event::Creates { what, .. } = event else {
        return None;
    };
    if what.filter.kind != Some(Noun::Token)
        || what.filter.alts.len() < 2
        || !what.filter.posts.is_empty()
    {
        return None;
    }
    what.filter
        .alts
        .iter()
        .map(|alt| match alt.as_slice() {
            [Adj::Subtype(s)] => Some(Effect::Create {
                who: who.clone(),
                n: Amount::ThatMany,
                token: Token {
                    tapped: false,
                    pt: None,
                    colors: Vec::new(),
                    types: vec![s.clone()],
                    keywords: Vec::new(),
                    abilities: Vec::new(),
                    named: Some(s.clone()),
                    attacking: None,
                },
            }),
            _ => None,
        })
        .collect()
}

/// What a card lets its player do with it from the opening hand (CR 103.6).
#[derive(Clone, Debug, PartialEq)]
pub enum OpeningHandAction {
    /// Begin the game with it on the battlefield (CR 103.6a): only if you
    /// aren't the starting player when `not_starting_player` (Gemstone
    /// Caverns), with `counters` on it, and `if_you_do` after.
    BeginOnBattlefield {
        not_starting_player: bool,
        counters: Option<(Amount, CounterKind)>,
        if_you_do: Vec<Effect>,
    },
    /// Reveal it (CR 103.6b), setting up what follows "If you do" (the
    /// delayed triggers at the first upkeep, the Chancellors').
    Reveal(Vec<Effect>),
}

/// The turn an extra turn comes after (CR 500.7).
#[derive(Clone, Debug, PartialEq)]
pub enum TurnRef {
    /// "after this one": the turn the effect happens in, and the reading
    /// when no turn is named.
    ThisOne,
    /// "After that turn": the turn just named, as the controlled player's
    /// next turn `ControlPlayerNextTurn` names (Emrakul, the Promised End).
    That,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tapped: bool,
    pub pt: Option<Pt>,
    pub colors: Vec<Color>,
    pub types: Vec<String>,
    pub keywords: Vec<Keyword>,
    pub abilities: Vec<Ability>,
    /// Treasure, Food, Clue and the other predefined tokens.
    pub named: Option<String>,
    /// "that's tapped and attacking that player or a planeswalker they control".
    pub attacking: Option<Recipient>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Restriction {
    /// "can attack as though it didn't have defender".
    AttacksDespiteDefender,
    CantBlock,
    CantAttack,
    CantAttackOrBlock,
    CantBeBlocked,
    CantBeBlockedExceptBy(Filter),
    /// "can't be attacked" (The Aetherspark): no creature may attack it, though
    /// it is a planeswalker (CR 508.1b).
    CantBeAttacked,
    CantBeCountered,
    /// "can't be sacrificed": no effect or cost may sacrifice it.
    CantBeSacrificed,
    /// "doesn't untap during your untap step", "… its controller's untap
    /// step": whose untap step, which for an Aura on another player's
    /// creature are not the same player.
    DoesntUntap(Player),
    AttacksEachCombat,
    AttacksThisTurn,
    BlocksEachCombat,
    CantBeTheTargetOf(Filter),
    CantBeRegenerated,
    CanBlockOnly(Filter),
    CantBeBlockedBy(Filter),
    CantBeBlockedByMoreThanOne,
    CanBlockAdditional(Amount),
    /// "can block any number of creatures": no bound on how many it
    /// blocks, where an X would be a number someone chooses.
    CanBlockAnyNumber,
    Unless(Box<Restriction>, Condition),
    /// "can't attack you unless their controller pays {2} for each …".
    UnlessPays {
        rule: Box<Restriction>,
        who: Player,
        cost: Vec<Cost>,
        per: Option<Objects>,
    },
    CantAttackPlayer(Player),
    /// "can't attack a player it has already attacked this turn" (Port
    /// Razer): only a player it hasn't attacked in an earlier combat this
    /// turn.
    CantAttackPlayerAttackedThisTurn,
    NoActivatedAbilities,
    MayNotUntap,
    DoesntUntapNext,
    /// "can't block creatures you control".
    CantBlockAny(Filter),
    /// "Target creature can't block this creature this turn", "Creatures
    /// with power less than this creature's power can't block it": a
    /// restriction on blocking one attacker (CR 509.1b).
    CantBlockObject(Objects),
    /// "can't attack you or planeswalkers you control": any of these.
    CantAttackAnyOf(Vec<Recipient>),
    /// "attack a player other than you if able".
    AttacksPlayerOtherThan(Player),
    /// "can't be activated unless they're mana abilities": the rule on a
    /// set of activated abilities that spares mana abilities.
    NoNonManaActivatedAbilities,
    CantEnterBattlefield,
    CantBeTurnedFaceUp,
    /// "can't have counters put on it".
    CantHaveCountersPut,
    /// "can't have hexproof or shroud", "can't have or gain flying": no
    /// effect can give the object these keywords while it applies.
    CantHave(Vec<Keyword>),
    /// "must be blocked if able": a requirement on the declaration of
    /// blockers (CR 509.1c).
    MustBeBlocked,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    When(Box<Event>),
    At(Step),
    /// "When ~ enters, if you cast it, …": checked as it triggers and again
    /// as it resolves (CR 603.4).
    InterveningIf(Box<Trigger>, Box<Condition>),
    /// "When this artifact enters and at the beginning of your upkeep": one
    /// ability with two trigger conditions (CR 603.1b), an event and a step.
    WhenAndAt(Box<Event>, Step),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// "When you spend this mana to cast …", "When that mana is spent": the
    /// delayed trigger a mana ability makes, on the mana it added (CR
    /// 106.6); no uses: spent on anything.
    ManaSpent(Vec<SpendUse>),
    Enters {
        what: Objects,
        under: Option<Player>,
        /// "enters untapped".
        state: Option<State>,
    },
    Dies(Objects),
    Leaves {
        what: Objects,
        from: Zone,
    },
    Attacks(Objects),
    Blocks(Objects),
    AttacksOrBlocks(Objects),
    BecomesBlocked(Objects),
    /// "deals combat damage to a player".
    DealsCombatDamageToPlayer(Objects),
    /// "deals combat damage to a player or planeswalker", "… to an
    /// opponent", "… to one or more players": a recipient other than "a
    /// player", which says who may be hit and how often it triggers.
    DealsCombatDamageTo {
        what: Objects,
        to: Recipient,
    },
    DealsDamage(Objects),
    IsDealtDamage(Objects),
    Casts {
        who: Player,
        what: Objects,
    },
    GainsLife(Player),
    /// "Whenever you roll one or more dice" (once per instruction to roll,
    /// the planar die among them, CR 706.7), "Whenever you roll a die"
    /// (`each_die`: once per die rolled).
    RollsDice {
        who: Player,
        each_die: bool,
    },
    LosesLife(Player),
    Draws(Player),
    Discards {
        who: Player,
        what: Objects,
    },
    Sacrifices {
        who: Player,
        what: Objects,
    },
    YouAttack,
    BecomesTapped(Objects),
    /// "Whenever this Vehicle becomes crewed": its crew ability resolved and
    /// it became an artifact creature (CR 702.122a).
    BecomesCrewed(Objects),
    /// "When this creature is turned face up", "Whenever a permanent is
    /// turned face up": by any means (CR 708.8: abilities relating to it
    /// entering don't trigger, since it already entered).
    TurnedFaceUp(Objects),
    BecomesTarget {
        what: Objects,
        of: Option<Objects>,
    },
    PutInto {
        what: Objects,
        to: Zone,
        from: Zone,
    },
    Cycles {
        who: Player,
        what: Option<Objects>,
    },
    CounterPutOn(Objects),
    /// "an opponent plays a land".
    PlaysLand(Player),
    Or(Vec<Event>),
    Copies {
        who: Player,
        what: Objects,
    },
    Creates {
        by: Actor,
        what: Objects,
        under: Option<Player>,
    },
    /// "one or more +1/+1 counters would be put on a creature you control".
    CountersPut {
        by: Option<Actor>,
        n: Quant,
        kind: Option<CounterKind>,
        on: Objects,
    },
    TappedForMana(Objects),
    /// "an opponent draws their second card each turn".
    DrawsNth {
        who: Player,
        n: u32,
    },
    /// "an opponent draws a card except the first one they draw in each of
    /// their draw steps".
    DrawsExceptFirstInDrawStep(Player),
    /// "that spell is countered".
    Countered(Objects),
    /// "you search your library".
    Searches {
        who: Player,
        zone: Zone,
    },
    /// "you tap a Swamp for mana".
    TapsForMana {
        who: Player,
        what: Objects,
    },
    /// "If you would draw a card while your library has no cards in it".
    While {
        event: Box<Event>,
        cond: Box<Condition>,
    },
    /// "deals damage to an opponent".
    DealsDamageTo {
        what: Objects,
        to: Vec<Recipient>,
    },
    /// "When this Class becomes level 2".
    BecomesLevel {
        what: Objects,
        n: u32,
    },
    /// "Whenever a player attacks one of your opponents", "Whenever an
    /// opponent attacks you": one or more creatures that player controls
    /// are declared as attackers (CR 508.3d), at `at` when it is said.
    /// "you attack" alone is `YouAttack`.
    PlayerAttacks {
        who: Player,
        at: Option<Player>,
    },
    /// "an opponent attacks you with two or more creatures"; `at` is who
    /// they attack, when the text says.
    AttacksWith {
        who: Player,
        what: Objects,
        at: Option<Player>,
    },
    /// "an opponent would begin an extra turn".
    BeginsExtraTurn(Player),
    /// "the twelfth hour counter is put on this artifact".
    NthCounterPutOn {
        n: u32,
        kind: CounterKind,
        on: Objects,
    },
    /// "enchanted player is attacked".
    IsAttacked(Player),
    /// "you put a Cave onto the battlefield", "you put a card into your hand".
    Puts {
        who: Player,
        what: Objects,
        to: Zone,
    },
    /// "you exile two cards with the same name".
    Exiles {
        who: Player,
        what: Objects,
    },
    /// "would deal noncombat damage to an opponent".
    DealsNoncombatDamageTo {
        what: Objects,
        to: Vec<Recipient>,
    },
    /// "attacks for the first time each turn".
    FirstTimeEachTurn(Box<Event>),
    /// "you tap a permanent for {C}".
    TapsFor {
        who: Player,
        what: Objects,
        mana: Mana,
    },
    /// "a land's ability causes you to add one or more mana of the chosen color".
    AddsMana {
        who: Player,
        by_ability_of: Objects,
        of: Palette,
    },
    /// "it wasn't cast this way": cast, by whoever.
    WasCast(Objects),
    /// "a permanent left the battlefield under your control": while that
    /// player controlled it.
    LeftUnderControlOf {
        what: Objects,
        who: Player,
    },
    /// "equipped creature deals combat damage": to anything.
    DealsCombatDamage(Objects),
    /// "a creature card is exiled".
    Exiled(Objects),
    /// "one or more noncreature permanents are returned to hand": to their
    /// owners' hands from the battlefield.
    ReturnedToHand(Objects),
    /// "a creature attacks you or a planeswalker you control".
    AttacksAnyOf {
        what: Objects,
        to: Vec<Recipient>,
    },
    /// "you activate a loyalty ability".
    Activates {
        who: Player,
        what: Objects,
    },
    /// "a land is tapped for two or more mana".
    TappedForAtLeast {
        what: Objects,
        n: u32,
    },
    /// "would enter and it wasn't cast": the event, when the condition holds
    /// of it.
    AndIf {
        event: Box<Event>,
        cond: Box<Condition>,
    },
    /// "When the creature an opponent controls dies this turn, if you
    /// control your commander, …": a delayed trigger's event with an
    /// intervening "if" (CR 603.4), checked as it triggers and again as it
    /// resolves. A printed trigger's is `Trigger::InterveningIf`.
    InterveningIf {
        event: Box<Event>,
        cond: Box<Condition>,
    },
    /// "you would draw one or more cards": one event however many, where
    /// "a card" is each draw.
    DrawsOneOrMore(Player),
    /// "this card is cast from exile".
    CastFrom {
        what: Objects,
        zone: Zone,
    },
    /// "a spell was warped": cast for that keyword's cost (CR 702.185c).
    CastUsing {
        what: Objects,
        keyword: String,
    },
    /// "a creature you control attacks alone": the only creature declared
    /// as an attacker (CR 506.5).
    AttacksAlone(Objects),
    /// "Whenever this creature attacks and isn't blocked", "attacks a player
    /// and isn't blocked": no creature was declared as its blocker (CR
    /// 509.3g), at the attack's recipient when one is said.
    AttacksUnblocked {
        what: Objects,
        at: Option<Recipient>,
    },
    /// "Whenever you exert a creature", "this creature hasn't been exerted
    /// this turn": the keyword action exert (CR 701.43a).
    Exerts {
        who: Player,
        what: Objects,
    },
    /// "you scry", "you surveil": a keyword action the player performs.
    Performs {
        who: Player,
        action: String,
    },
    /// "this creature transforms into ~".
    TransformsInto {
        what: Objects,
        into: Objects,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Upkeep(Whose),
    DrawStep(Whose),
    PrecombatMain(Whose),
    Combat(Whose),
    EndStep(Whose),
    EndOfCombat(Whose),
    /// "second main phase": the second of the turn, counted (CR 505.1b).
    SecondMain(Whose),
    /// "postcombat main phase": any main phase after combat (CR 505.1a).
    PostcombatMain(Whose),
    /// "your next main phase": whichever main phase comes first.
    Main(Whose),
    Untap(Whose),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Whose {
    Yours,
    Each,
    EachOpponent,
    /// "the end step": this turn's.
    The,
    /// "the next end step": the first one to come, perhaps this turn's.
    Next,
    /// "the next turn's upkeep": a step of the turn after this one.
    NextTurns,
    ThatPlayers,
    YourNext,
    /// "your first upkeep": of the game.
    YourFirst,
    /// "the first upkeep": the game's, the starting player's.
    First,
    EachOther,

    /// "at the beginning of that combat": the one an earlier effect made.
    That,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Static {
    /// "X can't be greater than the number of players in the game." as a
    /// spell's own paragraph: a bound on the X chosen as it is cast.
    X(XLimit),
    Modify {
        what: Objects,
        pt: Option<Pt>,
        has: Vec<Keyword>,
        /// "for each creature you control": how many times the bonus applies.
        per: Option<Amount>,
    },
    HasAbility {
        what: Objects,
        ability: Box<Ability>,
    },
    Restrict {
        what: Objects,
        rule: Restriction,
    },
    EntersTapped(Objects),
    EntersWithCounters {
        what: Objects,
        n: Amount,
        kind: CounterKind,
    },
    /// "This creature escapes with two +1/+1 counters on it": if it escaped
    /// (was cast with escape), it enters with those counters (CR 702.138c).
    EscapesWithCounters {
        what: Objects,
        n: Amount,
        kind: CounterKind,
    },
    CostChange {
        what: Objects,
        by: Mana,
        less: bool,
        per: Option<Objects>,
    },
    /// "Activated abilities of artifacts you control cost {1} less to
    /// activate", "Exhaust abilities of other permanents you control …":
    /// the cost of each such ability of those objects.
    AbilityCostChange {
        kind: AbilityKind,
        of: Objects,
        by: Mana,
        less: bool,
    },
    /// "Cycling abilities you activate cost {2} less to activate": every
    /// such ability that player activates, from any zone, apart from the
    /// abilities of some objects: a cycling card in hand has an owner and
    /// no controller, and only its owner activates it (CR 602.2, 113.8).
    ActivatedCostChange {
        kind: AbilityKind,
        who: Player,
        by: Mana,
        less: bool,
    },
    /// "You may play two additional lands on each of your turns."
    ExtraLand(Amount),
    /// "your opponents can't cast spells or activate abilities of artifacts".
    PlayerRestrict {
        who: Player,
        rules: Vec<PlayerRule>,
    },
    /// "Untap all permanents you control during each other player's untap step."
    UntapDuring {
        what: Objects,
        step: Step,
    },
    AsLongAs {
        cond: Condition,
        then: Vec<Static>,
    },
    SetPt {
        what: Objects,
        each_equal_to: Amount,
    },
    MaxHandSize,
    AdditionalCost(Vec<Cost>),
    AlternativeCost(Vec<Cost>),
    CanBeCommander,
    /// "A deck can have any number of cards named ~" (`None`), "… up to
    /// nine cards named ~": a deck-construction rule that replaces the
    /// limit of four (CR 100.2a) for this card.
    DeckCanHave(Option<u32>),
    Control(Objects),
    /// "You may exert this creature as it attacks. When you do, …": an
    /// optional cost to attack (CR 701.43d, 508.1g); `then` is the
    /// reflexive trigger printed with it, linked to it (CR 607.2h), and
    /// empty when there is none.
    ExertAsAttacks {
        what: Objects,
        then: Vec<Effect>,
    },
    /// "Once each turn, you may cast a spell from the top of your library",
    /// "Once during each of your turns, you may cast a creature spell from
    /// your graveyard": the permission used at most once a turn, in your
    /// turns only when `yours`.
    Once {
        yours: bool,
        then: Vec<Static>,
    },
    AsEnters {
        what: Objects,
        then: Vec<Effect>,
    },
    EntersTappedUnless {
        what: Objects,
        unless: Condition,
    },
    LookAtTopAnyTime,
    YouHave(Vec<Keyword>),
    SetPower {
        what: Objects,
        equal_to: Amount,
    },
    If {
        cond: Condition,
        then: Vec<Static>,
    },
    /// "You may cast noncreature spells from the top of your library. If you
    /// cast a spell this way, you may cast it as though it had flash": the
    /// permissions, with the rules for each `spell` cast through them,
    /// applied as it is cast (CR 601.3 for when it may be cast) or as it
    /// enters. A static `If` would hold while a condition is true, and
    /// "you cast a spell this way" is no state.
    PermissionRules {
        permissions: Vec<Static>,
        spell: Objects,
        rules: Vec<Static>,
    },
    /// "You may cast this card from your graveyard by paying {2}{W} rather
    /// than paying its mana cost", "You may cast artifact spells from your
    /// graveyard by paying 3 life in addition to paying their other costs":
    /// a permission to cast, from `zone` where one is said, with an
    /// alternative cost (`rather`, CR 118.9) or an additional one (CR 118.8).
    CastPaying {
        what: Objects,
        zone: Option<Zone>,
        cost: Vec<Cost>,
        rather: bool,
    },
    PlayFrom {
        verb: CastVerb,
        what: Objects,
        zone: Zone,
    },
    /// "If X would Y, Z instead" and friends, read as an effect pair.
    Replacement {
        event: Box<Event>,
        instead: Vec<Effect>,
    },
    /// "Each land is a Swamp in addition to its other land types."
    AlsoIs {
        what: Objects,
        types: TypeSet,
    },
    /// "you may cast this spell without paying its mana cost".
    CastWithoutPaying,
    /// "a triggered ability of … triggers an additional time", perhaps only
    /// when an event causes it: "an artifact or creature entering".
    AdditionalTrigger {
        of: Objects,
        cause: Option<Event>,
    },
    /// A static with an X defined after it: "costs {X} less …, where X is …".
    Where {
        inner: Box<Static>,
        x: Amount,
    },
    /// "You may have this creature enter as a copy of any creature on the
    /// battlefield, except …" (CR 707.9).
    EntersAsCopy {
        /// "You may have": the controller chooses whether it copies at all.
        may: bool,
        /// "enter tapped as a copy of".
        tapped: bool,
        what: Objects,
        of: Objects,
        except: Vec<Exception>,
    },
    /// "Enchanted land is a 3/3 black Ooze creature with haste. It's still
    /// a land.": what `Effect::Becomes` makes it, for as long as the static
    /// applies. The size sets base power and toughness (CR 613.4b), colors
    /// among `types` replace its colors, and `keeps` is what it retains
    /// (CR 205.1b).
    IsA {
        what: Objects,
        pt: Pt,
        types: Vec<Adj>,
        has: Vec<Keyword>,
        keeps: Keeps,
    },
    /// "Creatures you control are every creature type": types that replace
    /// the old, where `AlsoIs` adds to them.
    Is {
        what: Objects,
        types: TypeSet,
    },
    /// "The same is true for creature spells you control and …": the static
    /// before it, for more objects.
    SameFor(Vec<Objects>),
    /// "The escape cost is equal to the card's mana cost plus exile three
    /// other cards from your graveyard."
    KeywordCost {
        keyword: String,
        cost: Vec<Cost>,
    },
    /// "pay life equal to its mana value rather than pay its mana cost".
    PaysRather {
        what: Objects,
        cost: Vec<Cost>,
    },
    /// "Play with the top card of your library revealed."
    PlayWithTopRevealed(Player),
    /// "cost an additional 3 life to cast".
    AdditionalCostFor {
        what: Objects,
        cost: Vec<Cost>,
    },
    /// "Skip your draw step."
    SkipStep(Step),
    /// "Creatures you control can boast twice during each of your turns
    /// rather than once."
    ActivationsPerTurn {
        what: Objects,
        keyword: String,
        times: u32,
        during: Whose,
    },
    /// "You control your opponents": one player making another's choices.
    ControlPlayer(Player),
    /// "While an opponent is searching their library, they exile each card
    /// they find. You may play those cards for as long as they remain
    /// exiled, …": effects done while it holds. Each lasts as long as it
    /// says, not only while the condition holds: the play permission is
    /// given as the card is exiled and lasts while it stays exiled, and
    /// the spending rule with it, which names no end, while you may cast
    /// them.
    While {
        cond: Condition,
        then: Vec<Effect>,
    },
    /// "with base power and toughness 0/1".
    BasePt {
        what: Objects,
        pt: Pt,
    },
    /// "has base power and base toughness each equal to its mana value"
    /// (Opalescence), "is an artifact creature with power and toughness each
    /// equal to its mana value" (March of the Machines): base power and
    /// toughness set from a number, in layer 7b (CR 613.4b).
    BasePtEqual {
        what: Objects,
        each_equal_to: Amount,
    },
    /// "loses all other abilities, card types, and creature types".
    Loses {
        what: Objects,
        lost: Vec<Lost>,
    },
    /// "~ isn't a creature".
    IsNot {
        what: Objects,
        types: Vec<Adj>,
    },
    /// "Lands you control enter untapped."
    EntersUntapped(Objects),
    /// "You may cast spells as though they had flash."
    CastAsThoughFlash(Objects),
    /// "You may cast this spell as though it had flash if you pay {2} more
    /// to cast it": flash for an additional cost (CR 601.2f), which a
    /// condition would not ask the caster to pay.
    CastAsThoughFlashFor {
        what: Objects,
        cost: Vec<Cost>,
    },
    /// "You may activate abilities of creatures you control as though those
    /// creatures had haste."
    ActivateAsThoughHaste(Objects),
    /// "You may have this creature assign its combat damage as though it
    /// weren't blocked": while blocked, it may assign its combat damage to
    /// the player or permanent it attacks instead of its blockers (CR
    /// 510.1c), a choice the player assigning its damage makes (CR 510.1).
    /// "Enchanted creature's controller may have it assign …" and "For each
    /// creature you control, you may have that creature assign …" say the
    /// same of each object.
    AssignAsThoughUnblocked(Objects),
    /// "For each {B} in a cost, you may pay 2 life rather than pay that mana"
    /// (K'rrik): each such symbol in a cost its controller pays may be paid
    /// with that much life instead, as a Phyrexian symbol may (CR 107.4f).
    LifeForMana {
        mana: Mana,
        life: Amount,
    },
    /// "you may pay {0} rather than pay cycling costs" (New Perspectives), "…
    /// rather than pay the equip cost of the first equip ability you activate
    /// each turn" (Bruenor): a cost paid in place of a keyword
    /// ability's cost, with `first`, only for the first activation of it in
    /// each turn, or each of your turns.
    KeywordCostPaying {
        keyword: String,
        cost: Vec<Cost>,
        first: Option<EachTurn>,
    },
    /// "Humans you control have each of the chosen abilities" (Greymond): the
    /// keywords chosen with this object's `Choice::KeywordsAmong`.
    HasChosenAbilities(Objects),
    /// "Each nonland permanent you control is all colors."
    IsAllColors(Objects),
    /// "Enchanted creature gets +2/+2 and is goaded": goaded, a designation
    /// (CR 701.15b), by the controller of this object for as long as the
    /// static applies, not until that player's next turn as a one-shot
    /// goad is (CR 701.15a).
    Goaded(Objects),
    /// "Enchanted creature has protection from black. This effect doesn't
    /// remove this Aura.": the Auras named are not put into their owners'
    /// graveyards by the protection this static gives (CR 702.16n).
    ProtectionDoesntRemove(Objects),
    /// "This spell costs {1} less to cast for each card type among cards in
    /// your graveyard": a change per a count that is not of objects.
    CostChangePer {
        what: Objects,
        by: Mana,
        less: bool,
        per: Amount,
    },
    /// "Permanents entering don't cause abilities of permanents your
    /// opponents control to trigger"; any abilities when `of` is `None`.
    EnteringDoesntTrigger {
        what: Objects,
        of: Option<Objects>,
    },
    /// "Each opponent can cast spells only any time they could cast a sorcery."
    CastOnlyAsSorcery {
        who: Player,
        what: Objects,
    },
    /// "You may spend mana as though it were mana of any color to activate
    /// abilities of creatures you control."
    SpendAsAnyColor(Vec<SpendUse>),
    /// "mana of any type can be spent to cast those spells", "you may
    /// spend mana as though it were mana of any type to cast …" (CR 609.4b).
    SpendAsAnyType(Vec<SpendUse>),
    /// "This creature saddles Mounts and crews Vehicles as though its power
    /// were 2 greater": for those keyword abilities' costs only, which count
    /// the tapped creatures' power (CR 702.171a, 702.122a, 702.184a); each
    /// keyword with what it is said of.
    PowerGreaterFor {
        what: Objects,
        keywords: Vec<(String, Objects)>,
        by: Amount,
    },
    /// "have all activated abilities of all creature cards exiled with ~".
    HasAbilitiesOf {
        what: Objects,
        kind: AbilityKind,
        of: Objects,
    },
    /// "You may activate each of those abilities only once each turn." after
    /// a `HasAbilitiesOf` (The Enigma Jewel): a limit on each ability that
    /// static grants.
    GrantedAbilitiesLimit(Limit),
    /// "Prevent all noncombat damage that would be dealt to other creatures
    /// you control."
    PreventNoncombat(Vec<Recipient>),
    /// "Prevent all damage that would be dealt to this creature", "Prevent
    /// all combat damage that would be dealt to attacking artifact creatures
    /// you control": a prevention effect for as long as the static applies
    /// (CR 615.1a), of all damage or only combat damage, to and by what it
    /// says.
    PreventDamage {
        combat_only: bool,
        to: Option<Vec<Recipient>>,
        by: Option<Objects>,
    },
    /// "Spells and abilities your opponents control can't cause their
    /// controller to search their library."
    CantCauseSearch(Objects),
    /// "has vigilance if it's white, hexproof if it's blue, …": each keyword
    /// while its condition holds of the object.
    HasIf {
        what: Objects,
        pairs: Vec<(Vec<Keyword>, Condition)>,
    },
    /// "Your maximum hand size is five."
    MaxHandSizeIs {
        who: Player,
        n: u32,
    },
    /// "As this creature transforms into ~, …": applied while it transforms (CR 712.20).
    AsTransforms {
        what: Objects,
        into: Objects,
        then: Vec<Effect>,
    },
    /// "As an additional cost to cast this spell, you may …": a cost the
    /// caster may choose not to pay.
    OptionalAdditionalCost(Vec<Cost>),
    /// "As an additional cost to cast this spell, you may sacrifice one or
    /// more creatures. When you do, copy this spell for each creature
    /// sacrificed this way." (Plumb the Forbidden): the optional cost, and
    /// a reflexive trigger (CR 603.12) when it was paid as the spell was cast.
    OptionalAdditionalCostWhenPaid {
        cost: Vec<Cost>,
        then: Vec<Effect>,
    },
    /// "You may cast this card from your graveyard using its bestow ability."
    CastFromUsing {
        zone: Zone,
        keyword: String,
    },
    SetToughness {
        what: Objects,
        equal_to: Amount,
    },
    /// "this creature has the power, toughness, and creature types of the
    /// last creature card exiled with it. It's still a Shapeshifter."
    /// (Duplicant): those values of `what` are `of`'s. Its rulings: the power
    /// and toughness are base values (layer 7b, CR 613.4b), so counters and
    /// other effects still apply; the creature types replace its own
    /// (layer 4), but for the subtypes `still` keeps.
    HasValuesOf {
        what: Objects,
        values: Vec<CopiedValue>,
        of: Objects,
        still: Vec<Adj>,
    },
    /// "This creature enters prepared.": a preparation card (CR 722) whose
    /// copy of its inset spell may be cast while it is prepared.
    EntersPrepared(Objects),
    /// A "you may have" static followed by "When you do, …": a reflexive
    /// trigger on the choice (CR 603.12).
    Reflexive {
        inner: Box<Static>,
        then: Vec<Effect>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeSet {
    Listed(Vec<Adj>),
    /// "Nonbasic lands are Mountains": subtypes alone, which replace only
    /// the subtypes of their set; a Blood Mooned land is still a land, and
    /// still snow or legendary (CR 205.1a, 305.7). A color said with them,
    /// "is a black Dragon", replaces its colors (CR 105.3).
    Subtypes(Vec<Adj>),
    /// "the chosen type".
    Chosen,
    EveryBasicLandType,
    EveryCreatureType,
    /// "This land is every nonbasic land type" (Planar Nexus): each land
    /// type that is not basic (CR 205.3i).
    EveryNonbasicLandType,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    /// "If both targets are still legal as this ability resolves" (Goblin
    /// Welder): each of the ability's targets passes the check of CR
    /// 608.2b, where the rule alone resolves the ability if any one does.
    TargetsStillLegal,
    /// "If carnage gets more votes" (CR 701.38): that choice of a vote got
    /// more votes than the other.
    GetsMoreVotes(String),
    /// "the vote is tied": no choice of the vote got more votes.
    VoteTied,
    YouControl(Objects),
    /// "if that spell's mana cost or that ability's activation cost
    /// contains {X}" (Unbound Flourishing).
    CostHasX(Objects),
    YouDontControl(Objects),
    ItsYourTurn,
    /// "if it's the first combat phase of the turn": no combat phase came
    /// before this one this turn, where extra combat phases follow it (CR
    /// 500.8); `your_turn` when said "of your turn", which it must also be.
    FirstCombatPhase {
        your_turn: bool,
    },
    NotYourTurn,
    /// "during an opponent's turn", "it's an opponent's turn": apart from
    /// "not your turn", which a teammate's turn also is.
    OpponentsTurn,
    /// "during their turn": the turn of the player the trigger's event
    /// names (Unstable Glyphbridge's "an opponent casts a spell during
    /// their turn").
    TheirTurn,
    LifeAtLeast(u32),
    LifeAtMost(u32),
    CardsInHandAtMost(u32),
    Is {
        what: Objects,
        filter: Filter,
    },
    WasKicked,
    Threshold(Amount, Box<Objects>),
    YouAttackedThisTurn,
    Opponent(Box<Condition>),
    OpponentsAtLeast(u32),
    PlayerControls(Player, Objects),
    PlayerHas {
        who: Player,
        at_least: u32,
        what: Holding,
    },
    /// "if you cast it": the object cast, the card or the object a trigger
    /// named.
    YouCastIt(Objects),
    /// "if you cast it from your hand": by you, from that zone.
    YouCastItFrom {
        what: Objects,
        zone: Zone,
    },
    NoSpellsCastLastTurn,
    TwoSpellsCastByAPlayerLastTurn,
    Not(Box<Condition>),
    And(Vec<Condition>),
    /// "it has a +1/+1 counter on it", or any kind when `None`: "counters".
    HasCounter {
        what: Objects,
        kind: Option<CounterKind>,
    },
    /// "it's on the battlefield".
    In {
        what: Objects,
        zone: Zone,
    },
    YouAreStartingPlayer,
    /// "you created a token this turn".
    DidThisTurn(Box<Event>),
    /// "creatures you control have total power 10 or greater".
    Total {
        what: Objects,
        cmp: Compare,
    },
    /// "an opponent controls more lands than you".
    ControlsMore {
        who: Player,
        what: Filter,
        than: Player,
    },
    /// "you control a commander as you cast this spell".
    AsYouCastIt(Box<Condition>),
    /// "that spell is countered this way", "you cast a spell this way": an
    /// event the ability's own earlier words caused.
    ThisWay(Box<Event>),
    /// "X is 10 or more", "its power is greater than each other creature's power".
    AmountIs {
        n: Amount,
        op: Op,
        value: Amount,
    },
    /// "it has the same name as another creature you control or a creature
    /// card in your graveyard".
    SameName {
        what: Objects,
        as_any_of: Vec<Objects>,
    },
    /// "this spell was cast from a graveyard".
    CastFrom {
        what: Objects,
        zone: Zone,
    },
    /// "you attacked with two or more creatures this turn".
    AttackedWithThisTurn(Objects),
    /// "you attacked with creatures with total power 6 or greater this
    /// combat" (pack tactics): in this combat phase only.
    AttackedWithThisCombat(Objects),
    /// "the gift was promised".
    GiftPromised,
    /// "you cast this spell during your main phase".
    CastDuring(Step),
    /// "they're searching their libraries".
    Searching {
        who: Player,
        zone: Zone,
    },
    /// "this permanent is attached to a creature you control".
    AttachedTo {
        what: Objects,
        to: Objects,
    },
    /// "it's your first, second, or third turn of the game".
    YourNthTurn(Vec<u32>),
    /// "it's your main phase".
    InStep(Step),
    /// "no mana was spent to cast it".
    NoManaSpentToCast(Objects),
    /// "mana from a Treasure was spent to cast it": any of the mana paid
    /// was produced by such a source (CR 106.3).
    ManaFromSpent {
        from: Objects,
        on: SpentFor,
    },
    WasBargained,
    /// "you've cast another spell named ~ this game".
    DidThisGame(Box<Event>),
    /// "it was a creature", "it had counters on it": the condition as it
    /// held when the event happened, not as it holds now.
    Past(Box<Condition>),
    /// "an opponent has 10 or less life".
    PlayerHasAtMost {
        who: Player,
        at_most: u32,
        what: Holding,
    },
    /// "you have fewer than seven cards in hand": strictly fewer, kept
    /// apart from "six or fewer" because "the difference" after it is
    /// counted from the number said ([the-difference]).
    PlayerHasFewerThan {
        who: Player,
        n: u32,
        what: Holding,
    },
    /// "it has mana value 2 or less".
    Has {
        what: Objects,
        cmp: Compare,
    },
    /// "{G}{G} was spent to cast it".
    ManaSpent {
        mana: Mana,
        what: Objects,
    },
    /// "you have the city's blessing".
    CitysBlessing(Player),
    /// "a graveyard has twenty or more cards in it": some one zone of that
    /// kind, not all of them together.
    ZoneHasCards {
        zone: Zone,
        op: Op,
        n: u32,
    },
    /// "it's the second creature spell you cast this turn".
    NthSpellThisTurn {
        what: Objects,
        n: u32,
        spell: Filter,
        who: Player,
    },
    /// "it targets a blue spell".
    Targets {
        what: Objects,
        target: Objects,
    },
    /// "an opponent has more life than you".
    HasMore {
        who: Player,
        what: Holding,
        than: Player,
    },
    /// "this is the fourth time this ability has resolved this turn".
    NthResolution(u32),
    /// "the {1}{B} cost was paid": the alternative cost of that amount.
    CostPaid(Mana),
    /// "this spell's additional cost was paid".
    AdditionalCostPaid,
    /// "enchanted creature is face down".
    FaceDown(Objects),
    /// "you win the flip", "you lose the flip".
    WonFlip(bool),
    /// "it was kicked with its {G} kicker".
    KickedWith(Mana),
    Or(Vec<Condition>),
    /// "it entered from your library", "it entered under your control".
    Entered {
        what: Objects,
        from: Option<Zone>,
        under: Option<Player>,
    },
    /// "it wasn't cast": cast at all, by anyone.
    WasCast(Objects),
    /// "this creature attacked or blocked this combat" (Clockwork Beast),
    /// "… this turn": it was declared as an attacker or a blocker in that
    /// combat, or in any combat this turn.
    AttackedOrBlocked {
        what: Objects,
        this_combat: bool,
    },
    /// "an opponent protects it" (Portent Tracker): the player is the
    /// battle's protector (CR 310.9).
    Protects {
        who: Player,
        what: Objects,
    },
    /// "it wasn't cycled" (Abandoned Sarcophagus): it is not being
    /// discarded to pay a cycling cost (CR 702.29a).
    WasCycled(Objects),
    /// "there is an instant card and a sorcery card in your graveyard".
    ThereAre(Vec<Objects>),
    /// "If a creature card was exiled this way", "if an artifact is
    /// destroyed this way": one of those objects is among what this
    /// ability's verb acted on.
    DoneThisWay {
        what: Objects,
        how: Marked,
    },
    /// "you both own and control this land and a creature named …".
    OwnAndControl(Vec<Objects>),
    /// "damage is prevented this way".
    DamagePreventedThisWay,
    /// "you have max speed": that player's speed is 4 (CR 702.179e).
    MaxSpeed(Player),
    /// "you have an enduring story": a designation storied gives (CR
    /// 702.195b).
    EnduringStory(Player),
    /// "if a Dragon was beheld": the beheld object had the quality when it
    /// was beheld (CR 701.4b).
    Beheld(Objects),
    /// "this spell was cast using teamwork": its caster chose to pay that
    /// keyword's cost (CR 702.194b).
    CastUsing {
        what: Objects,
        keyword: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    Color,
    /// "chooses money, friends, or secrets" (Master of Ceremonies): one of
    /// these words, which have no rules meaning, as a vote's (CR 701.38b).
    Words(Vec<String>),
    /// "choose odd or even": a parity, zero being even.
    OddOrEven,
    CreatureType,
    CardType,
    /// "choose a permanent type": artifact, battle, creature, enchantment,
    /// land or planeswalker (CR 110.4).
    PermanentType,
    CardName,
    /// "choose a nonland card name".
    NonlandCardName,
    Number,
    Player,
    Opponent,
    /// "choose target artifact card in your graveyard".
    Objects(Objects),
    /// "choose target creature you control and target creature you don't
    /// control": two targets chosen at once, each its own target (CR
    /// 115.1), which later sentences tell apart by description ("the
    /// creature you control") or name together ("those creatures")
    /// ([choose-targets]).
    Targets(Vec<Objects>),
    /// "choose artifact, creature, enchantment, instant, or sorcery": one
    /// of these card types.
    CardTypeAmong(Vec<String>),
    /// "choose two abilities from among first strike, vigilance, and
    /// lifelink" (Greymond): that many different ones of those listed.
    KeywordsAmong {
        n: u32,
        among: Vec<Keyword>,
    },
    LandCardName,
    /// "choose target opponent": a target, so it can be illegal on
    /// resolution and hexproof stops it, where "choose an opponent" is not.
    TargetOpponent,
    TargetPlayer,
    /// "an artifact, a creature, an enchantment, and a planeswalker from
    /// among the nonland permanents they control": one of each.
    OneOfEach {
        picks: Vec<Filter>,
        among: Objects,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Holding {
    Life,
    CardsInHand,
    CardsInGraveyard,
    Opponents,
}

/// The basic land types (CR 205.3i), as a token's subtypes say them.
pub const BASIC_LAND_TYPES: [&str; 5] = ["plains", "island", "swamp", "mountain", "forest"];

/// "each turn" or "during each of your turns", after "the first …".
#[derive(Clone, Debug, PartialEq)]
pub enum EachTurn {
    Turn,
    YourTurn,
}

impl Ability {
    /// The effects an ability's text lists, where it lists some.
    pub fn effects_mut(&mut self) -> Option<&mut Vec<Effect>> {
        match self {
            Ability::Spell(es)
            | Ability::Activated { effect: es, .. }
            | Ability::Loyalty { effect: es, .. }
            | Ability::Triggered { effect: es, .. }
            | Ability::Spree { effect: es, .. }
            | Ability::Chapter { effect: es, .. } => Some(es),
            Ability::Labelled { ability, .. } => ability.effects_mut(),
            _ => None,
        }
    }
}
