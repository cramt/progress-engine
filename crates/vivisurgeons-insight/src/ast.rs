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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mana(pub Vec<String>);

#[derive(Clone, Debug, PartialEq)]
pub enum Limit {
    SorcerySpeed,
    OncePerTurn,
    OnlyDuringYourTurn,
    OnlyIf(Condition),
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
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Filter {
    /// An or of ands: "white or black creature" is `[[white, creature], [black, creature]]`.
    pub alts: Vec<Vec<Adj>>,
    /// What kind of object: "card", "spell", "token". `None` means a permanent
    /// named by its type, as in "target creature".
    pub kind: Option<Noun>,
    pub posts: Vec<Post>,
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
    SharesType,
    /// "other than ~".
    OtherThanSelf,
    Cast(Player),
    FromAmongThem,
    ExiledWith(Box<Objects>),
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
        under_your_control: bool,
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
    },
    ChooseNewTargets,
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
        dur: Duration,
    },
    BecomesCopy {
        what: Objects,
        of: Objects,
        dur: Duration,
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
    Where {
        effect: Box<Effect>,
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
        effect: Box<Effect>,
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum Recipient {
    Objects(Objects),
    Player(Player),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Choose {
    One,
    N(u32),
    OneOrBoth,
    OneOrMore,
    AnyNumber,
    UpTo(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ManaAdd {
    Symbols(Mana),
    Or(Vec<Mana>),
    AnyColor(Amount),
    AnyOneColor(Amount),
    AnyCombination(Amount),
    ChosenColor(Amount),
    CommanderIdentity(Amount),
    OpponentLandsCouldProduce(Amount),
    AmountOf { mana: Mana, n: Amount },
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
    BlocksEachCombat,
    CantBeTheTargetOf(Filter),
    CantBeRegenerated,
    CanBlockOnly(Filter),
    CantBeBlockedBy(Filter),
    CantBeBlockedByMoreThanOne,
    CanBlockAdditional(Amount),
    Unless(Box<Restriction>, Condition),
    NoActivatedAbilities,
    MayNotUntap,
    DoesntUntapNext,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    When(Event),
    At(Step),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Enters {
        what: Objects,
        under: Option<Player>,
    },
    Dies(Objects),
    LeavesBattlefield(Objects),
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum Whose {
    Yours,
    Each,
    EachOpponent,
    The,
    ThatPlayers,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Static {
    Modify {
        what: Objects,
        pt: Option<Pt>,
        has: Vec<Keyword>,
        per: Option<Objects>,
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
    ExtraLand,
    AsLongAs {
        cond: Condition,
        then: Vec<Static>,
    },
    SpendOnly(Objects),
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
        what: Objects,
        zone: Zone,
    },
    /// "If X would Y, Z instead" and friends, read as an effect pair.
    Replacement {
        event: Box<Event>,
        instead: Vec<Effect>,
    },
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
