# Dropped tag deduction — "which bird does this tag belong to?"

## The problem

A quail's NFC leg band sometimes falls off. The tag itself still works — you find it on
the floor of the breeding group's hutch. The bird is still right there in the group, just
unbanded now. The question is simple to ask and annoying to answer by eye: *which of these
birds does the dropped tag belong to, so I can re-attach it?*

This feature is **diagnosis only**. It reads the tag, looks at the birds present, and
narrows down whose tag it is. It does not write anything to the database, does not re-band,
and does not mint a new tag. You re-attach the existing tag yourself once you know who it
belongs to. The stored tag→bird mapping was always correct; the band just came off
physically.

It exists only for **breeding groups**, because that's where the deduction is actually
decisive (see "Why breeding groups only" below).

## The core idea: eliminate first, rank second

Bird attributes fall into two kinds, and they behave very differently in the logic.

A **hard attribute** is one where a mismatch is a *contradiction* — logically impossible —
so it eliminates a candidate outright. Sex is the cleanest example. If the dropped tag's
stored bird is female and the bird in front of you is unmistakably a crowing male, that tag
*cannot* be this bird. It's eliminated. No score, no maybe. Lineage works the same way: if
your records say the tag belongs to a Fernbank bird and the present bird is provably from a
different line, it's out.

A **soft attribute** is one where a mismatch is just *weak evidence*, not proof. Band color
is the one we use. You might mis-read a shade, or the band might be faded. A band-color
difference doesn't have the logical force to eliminate anyone, so it never does. Instead,
soft attributes *rank* the survivors — among the tags that passed the hard filter, the one
whose stored band color matches what you see is more likely, but you're not betting the
breeding records on it.

That's the whole design in one line: **hard attributes narrow the set by elimination, soft
attributes order what's left by plausibility.**

A "not sure" observation is always treated as no information — it never eliminates and never
penalizes. (Sex has a real `Unknown` value for exactly this reason.)

## The hard attributes here: sex and lineage

The schema only supports two true hard attributes:

- **Sex** — `Male` / `Female` / `Unknown`. An observed sex that contradicts the tag's
  stored sex eliminates that tag for that bird. `Unknown` eliminates nothing.
- **Lineage** — birds carry a set of lineages (many-to-many). A provable lineage mismatch
  between the present bird and the tag's stored bird disqualifies the tag.

(Breed and color don't exist as columns, so they aren't used. Size could one day be bucketed
from weight as another soft trait, but it isn't built yet.)

## The single-male short-circuit

A breeding group is exactly one male plus N females. That structure does a lot of work for
free:

- If the dropped tag's stored sex is **male**, you're instantly done. There's exactly one
  male in the group, so the tag is his — full stop, no deduction needed. ("The rooster lost
  his band" is the trivial common case.)
- If the dropped tag's stored sex is **female**, the male is eliminated from consideration
  immediately, and the real work is disambiguating among the females.

So the male is resolved in one step, and the candidate/propagation machinery only ever runs
over the female set.

## The soft trait: band color, ranked by Jaccard similarity

Once the hard filter has narrowed a bird's possible tags to the survivors, we rank those
survivors by how well their *observable* traits match — here, band color. The ranking uses
**Jaccard similarity**, which measures how much two sets overlap:

```
J(A, B) = |A ∩ B| / |A ∪ B|
```

In plain terms: of all the distinct traits that appear in *either* the observed bird or the
candidate tag's stored record, what fraction appear in *both*? It runs from 0 (nothing in
common) to 1 (identical).

A worked example. Treat each trait as a `key:value` token:

- Observed bird: `{band_color:yellow}`
- Tag A's stored bird: `{band_color:green}`
- Tag B's stored bird: `{band_color:yellow}`

For Tag A: shared tokens = none (0), union = `{band_color:yellow, band_color:green}` (2).
Jaccard = 0/2 = 0.0.

For Tag B: shared = `{band_color:yellow}` (1), union = `{band_color:yellow}` (1).
Jaccard = 1/1 = 1.0.

So Tag B ranks above Tag A. Neither was *eliminated* — band color is soft, so a mismatch
only lowers the rank — but B is the better match and the UI shows it first.

**Why a set measure instead of just counting matching fields?** The union in the denominator
handles missing observations gracefully. If you mark a trait "not sure," that token simply
isn't in the observed set — it doesn't count as a mismatch, it just doesn't contribute. A
bird you described with fewer traits is compared only on the traits you *did* note, without
being penalized for the blanks. That's the same "unknown is not a mismatch" rule the hard
filter follows.

> Note: this is a generic set-Jaccard helper, **not** the existing `compute_relatedness`
> function. That one is a genetic relatedness coefficient over lineage IDs and parents —
> it answers "how related are these two birds," which is a different question from "do these
> observable traits match." Don't conflate the two.

## Constraint propagation (the Sudoku step)

When several bands drop at once you have a set of tags against a set of birds, and solving
one can solve others. After building each female's candidate set, repeat until nothing
changes:

1. Any bird with exactly **one** surviving candidate is locked as `Resolved`.
2. That tag is removed from every *other* bird's candidate set.
3. A removal can create a new single-candidate bird — so loop again.

This is the same logic as filling in a forced cell in Sudoku, then using it to force the
next one.

The result carries a confidence flag worth paying attention to:

- **`Sole`** — this tag was the only candidate for the bird from the start. A clean,
  self-contained deduction.
- **`Forced`** — the bird had multiple candidates until *another* bird got locked and
  removed one. This conclusion is only as trustworthy as the chain of locks that produced
  it: if an upstream observation was wrong, a `Forced` result downstream can be wrong too.
  Worth a second look before trusting it.

## Outcomes

Each present bird ends up in one of three states:

- **`Resolved`** — exactly one tag is consistent (with `Sole` or `Forced` confidence).
  "This tag belongs to this bird. Re-attach it."
- **`Ambiguous`** — two or more tags still consistent, returned as a short list ranked
  best-first by band-color Jaccard. "Narrowed to these — check the distinguishing trait."
- **`NoCandidate`** — no dropped tag is consistent with this bird's hard attributes.

Any dropped tag that matches no present bird at all (e.g. the bird is actually missing, not
just unbanded) is reported separately as an unmatched tag.

## Why breeding groups only

This feature deliberately does not run on chick grow-out groups. In a hutch full of same-age,
same-sex, identical-looking juveniles, sex and lineage often can't eliminate anyone, so you'd
be leaning entirely on band color — the weak signal — and the deduction collapses into a
guess. Breeding groups are the opposite: small, sexed sets where the hard attributes have
real teeth (the single male resolves instantly; females split on lineage). It's the case
where the deduction is genuinely decisive, so it's the only case we support.
