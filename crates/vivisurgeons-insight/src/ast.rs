//! What an ability *is*, as far as its text says. Nothing here knows how to
//! run: an engine would interpret these, and the spike's question is only
//! whether text can be read into them without a hole.

use crate::lex::{Pt, Signed};

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
    Tap,
    Untap,
    Sacrifice(Objects),
    Discard(Objects),
    PayLife(Amount),
    Exile(Objects),
    RemoveCounters {
        n: Amount,
        kind: String,
        from: Objects,
    },
    PutCounters {
        n: Amount,
        kind: String,
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
        kind: String,
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpendUse {
    Cast(Objects),
    ActivateAbilityOf(Objects),
    /// "to activate abilities": of any object.
    ActivateAny,
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
    /// "the number of times it's been cast from the command zone this game".
    TimesCastFrom {
        what: Box<Objects>,
        zone: Zone,
    },
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
    /// "each time it has attacked this turn".
    TimesAttackedThisTurn(Box<Objects>),
    /// "up to two": the player picks any number to that bound.
    UpTo(Box<Amount>),
    Twice(Box<Amount>),
    /// "that many plus one".
    Plus(Box<Amount>, u32),
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
    /// "any amount of life": as much as the payer picks.
    AnyAmount,
    /// "the amount of {E} paid this way".
    EnergyPaidThisWay,
    /// "that number plus 1": the number the sentence just named.
    ThatNumber,
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
    /// "each player who searched a library this way".
    DidThisWay {
        who: Box<Player>,
        did: Box<Event>,
    },
    /// "up to one target player": a target the controller may leave unchosen.
    UpToOneTarget,
    /// "any number of target players' graveyards".
    AnyNumberOfTarget,
    /// "that player or that planeswalker's controller": whichever one the
    /// earlier choice made it.
    OneOf(Vec<Player>),
    /// "Those players" after "For each permanent destroyed this way, its
    /// controller …": every player the loop before named, all at once.
    Those(Box<Player>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Objects {
    pub quant: Quant,
    pub filter: Filter,
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
    /// The card itself.
    SelfRef,
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
    Counter(Option<String>),
    /// "one pile": a pile an earlier separation made.
    Pile,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Post {
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
    WithKeyword(String),
    WithoutKeyword(String),
    /// "with a +1/+1 counter on it", or of any kind when `None`: "with
    /// counters on them".
    WithCounter(Option<String>),
    /// "card named ~".
    Named(Box<Objects>),
    /// "spells your opponents cast that target this creature".
    Targets(Box<Objects>),
    /// "each spell you cast that's red or green".
    ThatIs(Filter),
    Attacking,
    Blocking,
    AttachedToIt,
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
    /// "other than ~", "other than this creature": the card, with the noun
    /// it was named by (as `SelfRef`'s filter keeps it), not an object of
    /// its own: a trigger's event still names one object.
    OtherThanSelf(Filter),
    Cast(Player),
    FromAmongThem,
    ExiledWith(Box<Objects>),
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
    /// "permanents that were cast this turn": by any player.
    WasCastThisTurn,
    /// "each opponent who lost life this turn".
    LostLifeThisTurn,
    /// "a land that could produce {C}".
    CouldProduce(Mana),
    /// "that is snow or could produce {C}": any one of these holds.
    Either(Vec<Post>),
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
        kind: String,
        on: Objects,
    },
    RemoveCounters {
        n: Amount,
        kind: String,
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
    ExtraTurn,
    RevealTop {
        who: Player,
        whose: Player,
        n: Amount,
    },
    CreateCopy {
        n: Amount,
        of: Objects,
        except: Vec<Exception>,
    },
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
        kind: String,
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
    /// "instead create one of each": n of each kind of token the replaced
    /// event named.
    CreateOfEach {
        who: Player,
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
        kind: String,
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
        kind: String,
    },
    /// "Distribute two +1/+1 counters among one or two target creatures".
    DistributeCounters {
        n: Amount,
        kind: String,
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
        kind: Option<String>,
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
        kind: String,
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
    },
    /// "return it to the battlefield with a -1/-1 counter on it": the moved
    /// permanent is given them as it enters (CR 122.6), not after.
    EntersWith {
        effect: Box<Effect>,
        n: Amount,
        kind: String,
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
    /// "After that turn, that player takes an extra turn."
    ExtraTurnAfterThat(Player),
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
    /// "Create a Monster Role token attached to it."
    CreateAttached {
        who: Player,
        n: Amount,
        token: Token,
        to: Objects,
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
        counters: Vec<(Amount, String)>,
        on: Objects,
    },
    /// "When a creature card is exiled this way, …": a reflexive triggered
    /// ability on what the ability's own words did (CR 603.12).
    WhenThisWay {
        event: Event,
        then: Vec<Effect>,
    },
    RemoveAllCounters(Objects),
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
    BecomesPrepared(Objects),
    /// "remove up to three counters from target creature": of any kinds,
    /// as the player picks.
    RemoveAnyCounters {
        n: Amount,
        from: Objects,
    },
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Characteristic {
    Abilities,
    CardTypes,
    CreatureTypes,
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
        kind: String,
        cond: Option<Box<Condition>>,
    },
    /// "his name is ~": the copy keeps this card's name.
    NameIsSelf,
    /// "he's a 4/4 Spider Human Hero in addition to his other types": a size
    /// and types on top of the copied ones.
    AlsoIsSized { pt: Pt, types: Vec<Adj> },
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
        kind: Option<String>,
        on: Objects,
    },
    TappedForMana(Objects),
    /// "an opponent draws their second card each turn".
    DrawsNth {
        who: Player,
        n: u32,
    },
    /// "When that mana is spent to cast a creature spell".
    ManaSpentToCast(Objects),
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
        kind: String,
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
    EachOther,

    /// "at the beginning of that combat": the one an earlier effect made.
    That,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Static {
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
        kind: String,
    },
    /// "This creature escapes with two +1/+1 counters on it": if it escaped
    /// (was cast with escape), it enters with those counters (CR 702.138c).
    EscapesWithCounters {
        what: Objects,
        n: Amount,
        kind: String,
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
    Control(Objects),
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
    /// "you may begin the game with ~ on the battlefield", the opening-hand
    /// rule (CR 103.6), with what follows "If you do".
    BeginOnBattlefield {
        what: Objects,
        counters: Option<(Amount, String)>,
        then: Vec<Effect>,
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
    /// "Each nonland permanent you control is all colors."
    IsAllColors(Objects),
    /// "Enchanted creature gets +2/+2 and is goaded": goaded, a designation
    /// (CR 701.15b), by the controller of this object for as long as the
    /// static applies, not until that player's next turn as a one-shot
    /// goad is (CR 701.15a).
    Goaded(Objects),
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
    /// "You may reveal this card from your opening hand. If you do, …".
    RevealFromOpeningHand(Vec<Effect>),
    /// "As an additional cost to cast this spell, you may …": a cost the
    /// caster may choose not to pay.
    OptionalAdditionalCost(Vec<Cost>),
    /// "You may cast this card from your graveyard using its bestow ability."
    CastFromUsing {
        zone: Zone,
        keyword: String,
    },
    SetToughness {
        what: Objects,
        equal_to: Amount,
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    YouControl(Objects),
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
        kind: Option<String>,
    },
    /// "it's on the battlefield".
    In {
        what: Objects,
        zone: Zone,
    },
    InOpeningHand(Objects),
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
    /// "choose odd or even": a parity, zero being even.
    OddOrEven,
    CreatureType,
    CardType,
    CardName,
    /// "choose a nonland card name".
    NonlandCardName,
    Number,
    Player,
    Opponent,
    /// "choose target artifact card in your graveyard".
    Objects(Objects),
    /// "choose artifact, creature, enchantment, instant, or sorcery": one
    /// of these card types.
    CardTypeAmong(Vec<String>),
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
