#!/usr/bin/env python3
"""
Seed QuailSync database with realistic multi-generational quail data.

Generates 5 generations across 3 bloodlines with breeding rotations,
realistic weight curves, clutch outcomes, mortality, and health events.

Usage:
    python scripts/seed_test_data.py                     # uses ./quailsync.db
    python scripts/seed_test_data.py path/to/quailsync.db
    python scripts/seed_test_data.py --reset              # wipe + re-seed
"""

import argparse
import math
import random
import sqlite3
import sys
from datetime import date, timedelta

# ── Reproducible randomness ──────────────────────────────────────────────────
random.seed(42)

# ── Timeline (relative to today) ────────────────────────────────────────────
TODAY = date.today()
G0_HATCH = TODAY - timedelta(days=180)   # ~6 months ago
G1_HATCH = TODAY - timedelta(days=150)   # ~5 months ago
G2_HATCH = TODAY - timedelta(days=105)   # ~3.5 months ago
G3_HATCH = TODAY - timedelta(days=60)    # ~2 months ago
G4_HATCH = TODAY - timedelta(days=14)    # ~2 weeks ago

# ── Bloodline definitions ────────────────────────────────────────────────────
BLOODLINES = [
    {"name": "Texas A&M",  "source": "Texas A&M University breeding program",
     "notes": "White feathered, fast growth, excellent egg production",
     "abbrev": "TX", "band_colors": ["Red", "Crimson", "Scarlet", "Ruby"]},
    {"name": "Pharaoh",    "source": "Heritage pharaoh line",
     "notes": "Wild-type plumage, hardy, good foragers",
     "abbrev": "PH", "band_colors": ["Green", "Lime", "Teal", "Olive"]},
    {"name": "Fernbank",   "source": "Fernbank breeding cooperative",
     "notes": "Tuxedo pattern, docile temperament, dual-purpose",
     "abbrev": "FB", "band_colors": ["Purple", "Violet", "Indigo", "Plum"]},
]

# ── Brooder definitions ─────────────────────────────────────────────────────
BROODERS = [
    {"name": "Brooder 1 - Texas",    "life_stage": "Chick"},
    {"name": "Brooder 2 - Pharaoh",  "life_stage": "Chick"},
    {"name": "Brooder 3 - Fernbank", "life_stage": "Chick"},
]

# Breeding rotation: each gen crosses line A males x line B females
# The offspring are assigned to line B's bloodline (maternal line).
CROSS_ROTATION = [
    # G0->G1: TX males x PH females, PH males x FB females, FB males x TX females
    [(0, 1), (1, 2), (2, 0)],
    # G1->G2: rotate — (TX×PH) males x FB females, (PH×FB) males x TX females, (FB×TX) males x PH females
    [(1, 2), (2, 0), (0, 1)],
    # G2->G3: rotate again
    [(2, 0), (0, 1), (1, 2)],
    # G3->G4: rotate
    [(0, 1), (1, 2), (2, 0)],
]

# ── Growth curve (Coturnix quail) ────────────────────────────────────────────
def weight_at_age(days, sex, jitter=True):
    """Realistic Coturnix weight curve. Returns weight in grams."""
    # Logistic growth: W(t) = W_max / (1 + exp(-k*(t - t_mid)))
    w_max = 250.0 if sex == "Female" else 200.0
    k = 0.08
    t_mid = 35  # inflection point ~5 weeks
    w_min = 7.0  # hatch weight

    w = w_min + (w_max - w_min) / (1 + math.exp(-k * (days - t_mid)))

    if jitter:
        # +/- 8% natural variation
        w *= random.uniform(0.92, 1.08)
    return round(w, 1)


# ── Health events ────────────────────────────────────────────────────────────
HEALTH_EVENTS = [
    "treated for bumblefoot",
    "eye discharge - monitored, resolved",
    "mild respiratory - isolated 3 days, recovered",
    "leg band replaced - original lost",
    "pecking injury on back - separated, healed",
    "slightly underweight - supplemental feed added",
    "crop issue - monitored, resolved in 2 days",
    "feather loss on neck - stress molt, recovered",
]

# ── Quail names (only ~20% of birds get named) ──────────────────────────────
QUAIL_NAMES = [
    "Nugget", "Pepper", "Cinnamon", "Clover", "Maple", "Hazel", "Ginger",
    "Dusty", "Cricket", "Pebble", "Sage", "Wren", "Finch", "Chip", "Rusty",
    "Dottie", "Speckle", "Bramble", "Fern", "Pippin", "Tango", "Mochi",
    "Butterscotch", "Acorn", "Juniper", "Hickory", "Sorrel", "Birch",
    "Marigold", "Thistle", "Jasper", "Olive", "Willow", "Basil", "Poppy",
    "Cedar", "Ember", "Flint", "Ivy", "Lark", "Moss", "Orchid", "Quill",
]
_name_idx = 0


def maybe_name():
    """Return a name for ~20% of birds, None otherwise."""
    global _name_idx
    if random.random() < 0.20 and _name_idx < len(QUAIL_NAMES):
        name = QUAIL_NAMES[_name_idx]
        _name_idx += 1
        return name
    return None


# ── Database helpers ─────────────────────────────────────────────────────────
def reset_tables(conn):
    """Delete all data from seeded tables (preserves schema)."""
    tables = [
        "chick_mortality_log", "chick_groups", "weight_records",
        "processing_records", "clutches", "breeding_pairs",
        "breeding_group_members", "breeding_groups",
        "birds", "brooders", "bloodlines",
    ]
    for t in tables:
        conn.execute(f"DELETE FROM {t}")
    # Reset autoincrement counters
    conn.execute("DELETE FROM sqlite_sequence WHERE name IN ({})".format(
        ",".join(f"'{t}'" for t in tables)
    ))
    conn.commit()
    print("[reset] Cleared all seeded tables")


def insert_bloodline(conn, bl):
    conn.execute(
        "INSERT INTO bloodlines (name, source, notes) VALUES (?, ?, ?)",
        (bl["name"], bl["source"], bl["notes"]),
    )
    return conn.execute("SELECT last_insert_rowid()").fetchone()[0]


def insert_brooder(conn, br, bloodline_id):
    conn.execute(
        "INSERT INTO brooders (name, bloodline_id, life_stage) VALUES (?, ?, ?)",
        (br["name"], bloodline_id, br["life_stage"]),
    )
    return conn.execute("SELECT last_insert_rowid()").fetchone()[0]


def insert_bird(conn, bird):
    name = maybe_name()
    notes = name  # use name as notes if named, else None
    conn.execute(
        """INSERT INTO birds
           (band_color, sex, bloodline_id, hatch_date, mother_id, father_id,
            generation, status, notes, nfc_tag_id)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
        (
            bird["band_color"],
            bird["sex"],
            bird["bloodline_id"],
            bird["hatch_date"].isoformat(),
            bird.get("mother_id"),
            bird.get("father_id"),
            bird["generation"],
            bird.get("status", "Active"),
            notes,
            bird["nfc_tag_id"],
        ),
    )
    return conn.execute("SELECT last_insert_rowid()").fetchone()[0]


def insert_weight(conn, bird_id, weight, record_date, notes=None):
    conn.execute(
        "INSERT INTO weight_records (bird_id, weight_grams, date, notes) VALUES (?, ?, ?, ?)",
        (bird_id, weight, record_date.isoformat(), notes),
    )


def insert_breeding_pair(conn, male_id, female_id, start_date, notes=None):
    conn.execute(
        "INSERT INTO breeding_pairs (male_id, female_id, start_date, notes) VALUES (?, ?, ?, ?)",
        (male_id, female_id, start_date.isoformat(), notes),
    )
    return conn.execute("SELECT last_insert_rowid()").fetchone()[0]


def insert_clutch(conn, pair_id, bloodline_id, eggs_set, set_date, status,
                  eggs_fertile=None, eggs_hatched=None, notes=None,
                  eggs_infertile=None, eggs_quit=None, eggs_stillborn=None,
                  eggs_damaged=None, hatch_notes=None):
    expected = set_date + timedelta(days=17)
    conn.execute(
        """INSERT INTO clutches
           (breeding_pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched,
            set_date, expected_hatch_date, status, notes,
            eggs_infertile, eggs_quit, eggs_stillborn, eggs_damaged, hatch_notes)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
        (
            pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched,
            set_date.isoformat(), expected.isoformat(), status, notes,
            eggs_infertile, eggs_quit, eggs_stillborn, eggs_damaged, hatch_notes,
        ),
    )
    return conn.execute("SELECT last_insert_rowid()").fetchone()[0]


def insert_chick_group(conn, clutch_id, bloodline_id, brooder_id,
                       initial_count, current_count, hatch_date, status="Active"):
    conn.execute(
        """INSERT INTO chick_groups
           (clutch_id, bloodline_id, brooder_id, initial_count, current_count,
            hatch_date, status)
           VALUES (?, ?, ?, ?, ?, ?, ?)""",
        (clutch_id, bloodline_id, brooder_id, initial_count, current_count,
         hatch_date.isoformat(), status),
    )
    return conn.execute("SELECT last_insert_rowid()").fetchone()[0]


def insert_mortality(conn, group_id, count, reason, log_date):
    conn.execute(
        "INSERT INTO chick_mortality_log (group_id, count, reason, date) VALUES (?, ?, ?, ?)",
        (group_id, count, reason, log_date.isoformat()),
    )


def insert_processing(conn, bird_id, reason, scheduled, status,
                      processed_date=None, final_weight=None, notes=None):
    conn.execute(
        """INSERT INTO processing_records
           (bird_id, reason, scheduled_date, processed_date,
            final_weight_grams, status, notes)
           VALUES (?, ?, ?, ?, ?, ?, ?)""",
        (bird_id, reason, scheduled.isoformat(),
         processed_date.isoformat() if processed_date else None,
         final_weight, status, notes),
    )


# ── Weight history generation ────────────────────────────────────────────────
def generate_weights(conn, bird_id, hatch_date, sex):
    """Generate weekly weight records from hatch to today."""
    d = hatch_date
    while d <= TODAY:
        age_days = (d - hatch_date).days
        w = weight_at_age(age_days, sex)
        insert_weight(conn, bird_id, w, d)
        d += timedelta(days=7)


# ── Health event generation ──────────────────────────────────────────────────
def maybe_health_event(conn, bird_id, hatch_date):
    """~5% chance of a health event note added as a weight record note."""
    if random.random() < 0.05:
        event_date = hatch_date + timedelta(days=random.randint(14, 90))
        if event_date <= TODAY:
            event = random.choice(HEALTH_EVENTS)
            insert_weight(conn, bird_id, weight_at_age(
                (event_date - hatch_date).days,
                random.choice(["Male", "Female"]),
            ), event_date, notes=event)


# ── Generation 0: Founders ───────────────────────────────────────────────────
def seed_founders(conn, bloodline_ids):
    """Create 10 founder birds per bloodline (6F, 4M). Returns dict of bird lists by bloodline index."""
    birds_by_line = {0: [], 1: [], 2: []}
    bird_num = 1

    for line_idx, bl_id in enumerate(bloodline_ids):
        bl = BLOODLINES[line_idx]
        sexes = ["Female"] * 6 + ["Male"] * 4
        random.shuffle(sexes)

        for i, sex in enumerate(sexes):
            # Stagger hatch dates over 2 weeks
            hatch = G0_HATCH + timedelta(days=random.randint(0, 14))
            band_color = bl["band_colors"][i % len(bl["band_colors"])]
            nfc = f"NFC-G0-{bl['abbrev']}-{bird_num:03d}"

            # Founder weight: adult range with variation
            adult_weight = random.uniform(180, 320)

            bid = insert_bird(conn, {
                "band_color": band_color,
                "sex": sex,
                "bloodline_id": bl_id,
                "hatch_date": hatch,
                "generation": 0,
                "status": "Active",
                "nfc_tag_id": nfc,
            })

            generate_weights(conn, bid, hatch, sex)
            maybe_health_event(conn, bid, hatch)

            birds_by_line[line_idx].append({
                "id": bid, "sex": sex, "bloodline_idx": line_idx,
                "hatch_date": hatch, "generation": 0,
            })
            bird_num += 1

    return birds_by_line


# ── Clutch simulation ────────────────────────────────────────────────────────
def simulate_clutch(conn, pair_id, bloodline_id, set_date, hatch_date):
    """Simulate a clutch with realistic outcomes. Returns (clutch_id, hatched_count)."""
    eggs_set = random.randint(8, 15)

    # Fertility: 75-85%
    fertility_rate = random.uniform(0.75, 0.85)
    eggs_fertile = round(eggs_set * fertility_rate)
    eggs_infertile = eggs_set - eggs_fertile

    # Hatch rate of fertile eggs: 70-80%
    hatch_rate = random.uniform(0.70, 0.80)
    eggs_hatched = round(eggs_fertile * hatch_rate)

    # Failure breakdown
    failed = eggs_fertile - eggs_hatched
    eggs_quit = random.randint(0, failed)
    eggs_stillborn = failed - eggs_quit
    eggs_damaged = random.randint(0, min(2, eggs_infertile))
    eggs_infertile = max(0, eggs_infertile - eggs_damaged)

    candling_7 = f"Day 7 candling: {eggs_fertile}/{eggs_set} showing veins, {eggs_infertile + eggs_damaged} clear"
    candling_14 = f"Day 14 candling: {eggs_fertile - eggs_quit} developing well, {eggs_quit} quitters removed"
    hatch_notes = f"Hatch day: {eggs_hatched} healthy chicks emerged"
    notes = f"{candling_7}; {candling_14}"

    clutch_id = insert_clutch(
        conn, pair_id, bloodline_id,
        eggs_set=eggs_set,
        set_date=set_date,
        status="Hatched",
        eggs_fertile=eggs_fertile,
        eggs_hatched=eggs_hatched,
        eggs_infertile=eggs_infertile,
        eggs_quit=eggs_quit,
        eggs_stillborn=eggs_stillborn,
        eggs_damaged=eggs_damaged,
        notes=notes,
        hatch_notes=hatch_notes,
    )

    return clutch_id, eggs_hatched


# ── Breed a generation ───────────────────────────────────────────────────────
def breed_generation(conn, gen_num, parent_birds, bloodline_ids, hatch_date, brooder_ids):
    """Cross bloodlines per rotation, create clutches + offspring birds.

    Returns new birds_by_line dict for use as parents of next generation.
    """
    crosses = CROSS_ROTATION[gen_num - 1]
    new_birds = {0: [], 1: [], 2: []}
    bird_num_base = gen_num * 100
    bird_counter = [0]

    for sire_line, dam_line in crosses:
        sire_birds = [b for b in parent_birds[sire_line] if b["sex"] == "Male"]
        dam_birds = [b for b in parent_birds[dam_line] if b["sex"] == "Female"]

        if not sire_birds or not dam_birds:
            continue

        # Offspring go to dam's bloodline
        offspring_line = dam_line
        offspring_bl_id = bloodline_ids[offspring_line]
        bl = BLOODLINES[offspring_line]

        # 3-4 clutches per cross
        n_clutches = random.randint(3, 4)
        for c in range(n_clutches):
            sire = random.choice(sire_birds)
            dam = random.choice(dam_birds)

            # Set date is 17 days before hatch
            set_date = hatch_date - timedelta(days=17) + timedelta(days=random.randint(-3, 3))
            actual_hatch = hatch_date + timedelta(days=random.randint(-1, 1))

            pair_id = insert_breeding_pair(
                conn, sire["id"], dam["id"], set_date,
                notes=f"G{gen_num} cross: {BLOODLINES[sire_line]['abbrev']} x {BLOODLINES[dam_line]['abbrev']}",
            )

            clutch_id, hatched = simulate_clutch(
                conn, pair_id, offspring_bl_id, set_date, actual_hatch,
            )

            # Determine chick survival (85-95% of hatched survive first week)
            survival_rate = random.uniform(0.85, 0.95)
            survived = max(1, round(hatched * survival_rate))
            lost = hatched - survived

            # For G4 (current chicks), create chick groups in brooders
            if gen_num == 4:
                brooder_id = brooder_ids[offspring_line]
                group_id = insert_chick_group(
                    conn, clutch_id, offspring_bl_id, brooder_id,
                    initial_count=hatched, current_count=survived,
                    hatch_date=actual_hatch, status="Active",
                )
                if lost > 0:
                    insert_mortality(
                        conn, group_id, lost,
                        random.choice(["weak at hatch", "failure to thrive", "trampled"]),
                        actual_hatch + timedelta(days=random.randint(1, 5)),
                    )
                # G4 chicks don't get individual bird records yet (not graduated)
                continue

            # For older generations, create individual bird records
            sexes_pool = (["Female"] * max(1, survived // 2) +
                          ["Male"] * max(1, survived - survived // 2))
            random.shuffle(sexes_pool)
            # For G3, some birds are still too young to sex
            if gen_num == 3:
                sexes_pool = [s if random.random() > 0.3 else "Unknown" for s in sexes_pool]

            # Also create graduated chick groups for older gens
            if gen_num <= 3:
                status = "Graduated" if gen_num < 3 else "Active"
                brooder_id = brooder_ids[offspring_line]
                group_id = insert_chick_group(
                    conn, clutch_id, offspring_bl_id, brooder_id,
                    initial_count=hatched, current_count=survived,
                    hatch_date=actual_hatch, status=status,
                )
                if lost > 0:
                    insert_mortality(
                        conn, group_id, lost,
                        random.choice(["weak at hatch", "failure to thrive"]),
                        actual_hatch + timedelta(days=random.randint(1, 5)),
                    )

            for j in range(min(survived, len(sexes_pool))):
                bird_counter[0] += 1
                sex = sexes_pool[j]
                band_color = bl["band_colors"][j % len(bl["band_colors"])]
                nfc = f"NFC-G{gen_num}-{bl['abbrev']}-{bird_num_base + bird_counter[0]:03d}"

                # Determine status: most active, some deceased/culled
                status = "Active"
                roll = random.random()
                if gen_num < 3:  # only older birds get culled/deceased
                    if roll < 0.07:
                        status = "Deceased"
                    elif roll < 0.12:
                        status = "Culled"

                bid = insert_bird(conn, {
                    "band_color": band_color,
                    "sex": sex,
                    "bloodline_id": offspring_bl_id,
                    "hatch_date": actual_hatch,
                    "mother_id": dam["id"],
                    "father_id": sire["id"],
                    "generation": gen_num,
                    "status": status,
                    "nfc_tag_id": nfc,
                })

                # Generate weight history
                effective_sex = sex if sex != "Unknown" else random.choice(["Male", "Female"])
                generate_weights(conn, bid, actual_hatch, effective_sex)
                maybe_health_event(conn, bid, actual_hatch)

                # Create processing record for culled birds
                if status == "Culled":
                    reason = random.choice(["ExcessMale", "LowWeight", "PoorGenetics"])
                    cull_date = actual_hatch + timedelta(days=random.randint(42, 70))
                    final_w = weight_at_age((cull_date - actual_hatch).days, effective_sex)
                    insert_processing(
                        conn, bid, reason, cull_date, "Completed",
                        processed_date=cull_date, final_weight=final_w,
                        notes=f"G{gen_num} cull - {reason.lower()}",
                    )

                new_birds[offspring_line].append({
                    "id": bid, "sex": sex, "bloodline_idx": offspring_line,
                    "hatch_date": actual_hatch, "generation": gen_num,
                })

    return new_birds


# ── Brooder readings (recent telemetry) ──────────────────────────────────────
def seed_brooder_readings(conn, brooder_ids):
    """Generate 2 hours of brooder readings (every 5 seconds = 1440 readings per brooder)."""
    for i, bid in enumerate(brooder_ids):
        # Each brooder has slightly different baseline
        base_temp = 97.5 + i * 0.5  # 97.5, 98.0, 98.5
        base_hum = 50.0 + i * 2     # 50, 52, 54

        now_ts = TODAY
        # Generate readings for last 2 hours (every 30s for manageable count)
        for minute_offset in range(120):
            ts = f"{now_ts.isoformat()}T{10 + minute_offset // 60:02d}:{minute_offset % 60:02d}:00Z"
            temp = base_temp + random.uniform(-1.5, 1.5)
            hum = base_hum + random.uniform(-3, 3)
            conn.execute(
                "INSERT INTO brooder_readings (temperature, humidity, timestamp, brooder_id) VALUES (?, ?, ?, ?)",
                (round(temp, 1), round(hum, 1), ts, bid),
            )
    conn.commit()


# ── Main seed logic ──────────────────────────────────────────────────────────
def seed(conn):
    conn.execute("PRAGMA foreign_keys = ON")

    # 1. Bloodlines
    print("[seed] Creating 3 bloodlines...")
    bloodline_ids = []
    for bl in BLOODLINES:
        bl_id = insert_bloodline(conn, bl)
        bloodline_ids.append(bl_id)
        print(f"  bloodline #{bl_id}: {bl['name']}")

    # 2. Brooders
    print("[seed] Creating 3 brooders...")
    brooder_ids = []
    for i, br in enumerate(BROODERS):
        br_id = insert_brooder(conn, br, bloodline_ids[i])
        brooder_ids.append(br_id)
        print(f"  brooder #{br_id}: {br['name']}")

    # 3. Generation 0 — Founders
    print("[seed] Generation 0: 30 founders (10 per line)...")
    birds = seed_founders(conn, bloodline_ids)
    for line_idx in range(3):
        males = sum(1 for b in birds[line_idx] if b["sex"] == "Male")
        females = sum(1 for b in birds[line_idx] if b["sex"] == "Female")
        print(f"  {BLOODLINES[line_idx]['name']}: {males}M / {females}F")

    # 4. Generations 1-4
    hatch_dates = [G1_HATCH, G2_HATCH, G3_HATCH, G4_HATCH]
    parents = birds
    total_birds = 30
    total_clutches = 0

    for gen in range(1, 5):
        print(f"[seed] Generation {gen}: breeding + hatching...")
        parents = breed_generation(
            conn, gen, parents, bloodline_ids, hatch_dates[gen - 1], brooder_ids,
        )
        gen_count = sum(len(parents[i]) for i in range(3))
        total_birds += gen_count
        # Count clutches for this gen
        clutch_count = conn.execute(
            "SELECT COUNT(*) FROM clutches c JOIN breeding_pairs bp ON c.breeding_pair_id = bp.id "
            "JOIN birds b ON bp.male_id = b.id WHERE b.generation = ?",
            (gen - 1,)
        ).fetchone()[0]
        total_clutches += clutch_count
        print(f"  {gen_count} birds from {clutch_count} clutches")

    # 5. Brooder readings
    print("[seed] Generating brooder telemetry readings...")
    seed_brooder_readings(conn, brooder_ids)

    conn.commit()

    # ── Summary ──────────────────────────────────────────────────────────────
    print("\n" + "=" * 50)
    print("SEED COMPLETE")
    print("=" * 50)

    stats = {
        "bloodlines": conn.execute("SELECT COUNT(*) FROM bloodlines").fetchone()[0],
        "brooders": conn.execute("SELECT COUNT(*) FROM brooders").fetchone()[0],
        "birds": conn.execute("SELECT COUNT(*) FROM birds").fetchone()[0],
        "active": conn.execute("SELECT COUNT(*) FROM birds WHERE status='Active'").fetchone()[0],
        "deceased": conn.execute("SELECT COUNT(*) FROM birds WHERE status='Deceased'").fetchone()[0],
        "culled": conn.execute("SELECT COUNT(*) FROM birds WHERE status='Culled'").fetchone()[0],
        "weights": conn.execute("SELECT COUNT(*) FROM weight_records").fetchone()[0],
        "pairs": conn.execute("SELECT COUNT(*) FROM breeding_pairs").fetchone()[0],
        "clutches": conn.execute("SELECT COUNT(*) FROM clutches").fetchone()[0],
        "chick_groups": conn.execute("SELECT COUNT(*) FROM chick_groups").fetchone()[0],
        "mortality_events": conn.execute("SELECT COUNT(*) FROM chick_mortality_log").fetchone()[0],
        "processing": conn.execute("SELECT COUNT(*) FROM processing_records").fetchone()[0],
        "readings": conn.execute("SELECT COUNT(*) FROM brooder_readings").fetchone()[0],
    }

    for key, val in stats.items():
        print(f"  {key:20s} {val:>6,}")

    # Generation breakdown
    print("\nBirds by generation:")
    for g in range(5):
        count = conn.execute("SELECT COUNT(*) FROM birds WHERE generation=?", (g,)).fetchone()[0]
        print(f"  G{g}: {count} birds")

    # Chick group summary
    print("\nChick groups:")
    groups = conn.execute(
        "SELECT cg.id, b.name, cg.initial_count, cg.current_count, cg.status "
        "FROM chick_groups cg LEFT JOIN brooders b ON cg.brooder_id = b.id ORDER BY cg.id"
    ).fetchall()
    for g in groups:
        print(f"  Group #{g[0]} in {g[1] or 'no brooder'}: {g[3]}/{g[2]} alive ({g[4]})")


def main():
    parser = argparse.ArgumentParser(description="Seed QuailSync database with test data")
    parser.add_argument("db_path", nargs="?", default="quailsync.db",
                        help="Path to SQLite database (default: quailsync.db)")
    parser.add_argument("--reset", action="store_true",
                        help="Delete existing seed data before re-seeding")
    args = parser.parse_args()

    print(f"[seed] Opening database: {args.db_path}")
    conn = sqlite3.connect(args.db_path)

    # Check if database has required tables
    tables = [r[0] for r in conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table'"
    ).fetchall()]
    if "birds" not in tables:
        print("[error] Database does not have QuailSync schema. Run the server first to create tables.")
        sys.exit(1)

    # Check if data already exists
    existing = conn.execute("SELECT COUNT(*) FROM bloodlines").fetchone()[0]
    if existing > 0 and not args.reset:
        print(f"[warn] Database already has {existing} bloodlines. Use --reset to wipe and re-seed.")
        sys.exit(0)

    if args.reset:
        reset_tables(conn)

    seed(conn)
    conn.close()
    print("\n[done]")


if __name__ == "__main__":
    main()
