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
    LevelHeader {
        from: u32,
        to: Option<u32>,
    },
    LevelPt(Pt),
    Chapter {
        chapters: Vec<u32>,
        effect: Vec<Effect>,
    },
    /// An ability word or a flavour word: a label with no rules meaning.
    Labelled {
        label: String,
        ability: Box<Ability>,
    },
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
    /// "Gift a card": what the promised opponent gets.
    Gift(Gift),
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
    /// "This ability costs {1} less to activate for each …".
    CostChange {
        by: Mana,
        less: bool,
        per: Option<Objects>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpendUse {
    Cast(Objects),
    ActivateAbilityOf(Objects),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Amount {
    N(u32),
    X,
    ThatMany,
    CountOf(Box<Objects>),
    PowerOf(Box<Objects>),
    ToughnessOf(Box<Objects>),
    ManaValueOf(Box<Objects>),
    LifeLost,
    Damage,
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
}

/// A set of colors a rule draws from, named by what decides it.
#[derive(Clone, Debug, PartialEq)]
pub enum Palette {
    Listed(Vec<Color>),
    /// "the chosen color", or "that color" after a choice or inside a
    /// "for each color".
    Chosen,
    CommanderIdentity,
    /// "the exiled card's colors", "among legendary creatures you control".
    ColorsOf(Box<Objects>),
    /// "that a land an opponent controls could produce".
    CouldProduce(Box<Objects>),
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
    Each,
    All,
    One,
    N(Amount),
    OneOrMore,
    AnyNumber,
    UpTo(Amount),
    Another,
    AnotherTarget,
    /// The card itself.
    SelfRef,
    /// "it", "them", "that creature", "those cards": an earlier noun.
    Antecedent,
    Enchanted,
    Equipped,
    /// A bare plural: "Creatures you control get +1/+1".
    Plural,
    Other,
    AtLeast(Amount),
    AtMost(Amount),
    /// "the rest": what an earlier choice left.
    Rest,
    /// "this creature or another Ally you control": itself, or one the filter names.
    SelfOrAnother,
    /// "this creature and other Vampire creatures you control": the one
    /// object, and every other the filter names.
    WithOthers(Box<Objects>),
    /// "your commander": the one a player has, by a possessive.
    Possessive(Player),
    /// "their first noncreature spell each turn".
    NthEachTurn {
        n: u32,
        of: Player,
    },
    /// "the next spell you cast this turn".
    Next,
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum Post {
    Controller(Player),
    NotControlledBy(Player),
    Owner(Player),
    In(Zone),
    With(Compare),
    WithKeyword(String),
    WithoutKeyword(String),
    WithCounter(String),
    Named,
    Attacking,
    Blocking,
    AttachedToIt,
    OfChosenType,
    /// "other than ~".
    OtherThanSelf,
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
    /// "spell you've cast this turn".
    CastThisTurn(Player),
    /// "with mana cost {0} or {1}".
    ManaCost(Vec<Mana>),
    AttackingPlayer(Player),
    /// "counter on ~".
    On(Box<Objects>),
    /// "the exiled card": an object an earlier verb picked out.
    Marked(Marked),
    PutOntoBattlefieldWith(Box<Objects>),
    /// "cards they exiled this way": marked by an earlier verb of one player.
    MarkedBy {
        how: Marked,
        by: Player,
    },
    /// "cards in your hand drawn this turn".
    DrawnThisTurn,
    /// "each opponent who doesn't": who did not do what an earlier "may" offered.
    WhoDoesnt,
    /// "that aren't on the battlefield".
    NotIn(Zone),
    /// "lands with different names".
    DifferentNames,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marked {
    Exiled,
    Chosen,
    Revealed,
    Sacrificed,
    Copied,
    Returned,
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
    Library(Player),
    TopOfLibrary(Player),
    BottomOfLibrary(Player),
    OwnersHand,
    OwnersGraveyard,
    OwnersLibraryTop,
    OwnersLibraryBottom,
    Battlefield,
    Exile,
    Stack,
    CommandZone,
    Anywhere,
    /// "anywhere other than the battlefield".
    AnywhereOtherThan(Box<Zone>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Duration {
    UntilEndOfTurn,
    ThisTurn,
    UntilYourNextTurn,
    WhileSelfOnBattlefield,
    Permanent,
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
    /// "When you do, …": a reflexive triggered ability (CR 603.12), which goes
    /// on the stack, unlike "If you do".
    WhenYouDo(Vec<Effect>),
    /// A modal spell's mode with a flavour label: "Sell Contraband — …".
    Labelled {
        label: String,
        then: Vec<Effect>,
    },
    /// "Change the target of target spell or ability with a single target."
    ChangeTarget(Objects),
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
    /// "you don't lose this mana as steps and phases end".
    KeepsMana {
        who: Player,
        mana: KeptMana,
    },
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
    /// The new types replace the old.
    Nothing,
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
    /// "it has haste".
    Has(Vec<Keyword>),
    /// "it enters with an additional +1/+1 counter on it if it's a creature".
    EntersWithCounters {
        n: Amount,
        kind: String,
        cond: Option<Box<Condition>>,
    },
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
    /// may choose both instead."
    Upgrade {
        base: Box<Choose>,
        cond: Box<Condition>,
        to: Box<Choose>,
    },
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
    CantBlock,
    CantAttack,
    CantAttackOrBlock,
    CantBeBlocked,
    CantBeBlockedExceptBy(Filter),
    CantBeCountered,
    DoesntUntap,
    AttacksEachCombat,
    AttacksThisTurn,
    BlocksEachCombat,
    CantBeTheTargetOf(Filter),
    CantBeRegenerated,
    CanBlockOnly(Filter),
    CantBeBlockedBy(Filter),
    CantBeBlockedByMoreThanOne,
    CanBlockAdditional(Amount),
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    When(Box<Event>),
    At(Step),
    /// "When ~ enters, if you cast it, …": checked as it triggers and again
    /// as it resolves (CR 603.4).
    InterveningIf(Box<Trigger>, Box<Condition>),
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
    DealsCombatDamageToPlayer(Objects),
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
    LandfallPlays(Player),
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Upkeep(Whose),
    DrawStep(Whose),
    PrecombatMain(Whose),
    Combat(Whose),
    EndStep(Whose),
    EndOfCombat(Whose),
    SecondMain(Whose),
    /// "your next main phase": whichever main phase comes first.
    Main(Whose),
    Untap(Whose),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Whose {
    Yours,
    Each,
    EachOpponent,
    The,
    ThatPlayers,
    YourNext,
    EachOther,
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
    CostChange {
        what: Objects,
        by: Mana,
        less: bool,
        per: Option<Objects>,
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeSet {
    Listed(Vec<Adj>),
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
    NotYourTurn,
    LifeAtLeast(u32),
    LifeAtMost(u32),
    CardsInHandAtMost(u32),
    Is {
        what: Objects,
        filter: Filter,
    },
    WasKicked,
    Threshold(Amount, Box<Objects>),
    Graveyard(Amount, Filter),
    YouAttackedThisTurn,
    Opponent(Box<Condition>),
    OpponentsAtLeast(u32),
    PlayerControls(Player, Objects),
    PlayerHas {
        who: Player,
        at_least: u32,
        what: Holding,
    },
    YouCastIt,
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
    /// "any of that mana is spent on a creature spell".
    ManaSpentOn(Objects),
    /// "you attacked with two or more creatures this turn".
    AttackedWithThisTurn(Objects),
    /// "the gift was promised".
    GiftPromised,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    Color,
    CreatureType,
    CardType,
    CardName,
    Number,
    Player,
    Opponent,
    /// "choose target artifact card in your graveyard".
    Objects(Objects),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Holding {
    Life,
    CardsInHand,
    CardsInGraveyard,
    Opponents,
}
