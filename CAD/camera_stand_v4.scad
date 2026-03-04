// QuailSync Camera Stand v4 - Back-Loading Design
//
// Camera slides in from the back into a snug 3-sided pocket
// A back plate snaps on with hook clips to secure it
// Ribbon cable exits through slot in back plate
// Lens faces forward through hole in front wall
//
// Much more practical: easy to load, secure hold, easy to remove
//
// ArduCam Module 3: 25mm x 24mm x 12.4mm
// Print: PLA, 0.2mm layers, 20% infill, no supports

$fn = 80;

// === CAMERA ===
cam_w = 25.0;       // actual width
cam_h = 24.0;       // actual height  
cam_d = 12.4;       // actual depth
cam_lens_d = 10;    // lens barrel diameter
tol = 0.3;          // per-side tolerance (snug fit)

// Effective pocket dimensions
pocket_w = cam_w + tol*2;  // 25.6
pocket_h = cam_h + tol*2;  // 24.6
pocket_d = cam_d + tol;    // 12.7 (open back, so only need front tol)

// === STAND ===
base_w = 70;
base_d = 50;
base_thick = 4;
base_r = 4;

col_w = pocket_w + 7;  // column width
col_d = 18;             // column depth (front-to-back)
col_h = 45;             // height to bottom of cradle

wall = 2.5;             // wall thickness
floor_t = 2.5;          // cradle floor

tilt_angle = 3;

// Cable
cable_w = 17;
cable_h = 3;

// Weight pockets
weight_d = 16;
weight_depth = 2.5;

// Hook clips on cradle for back plate
hook_w = 5;             // width of each hook
hook_thick = 1.8;       // hook material thickness
hook_overhang = 1.5;    // how far hook extends inward
hook_gap = 0.3;         // clearance

// Back plate
bp_thick = 2.5;
bp_tab_h = 4;           // bottom locating tab height

// Grip ribs inside pocket (prevent rattle)
rib_h = 0.4;            // rib protrusion height
rib_w = 1.0;            // rib width

// === HELPERS ===
module rounded_rect(w, d, h, r) {
    hull() {
        for (x = [r, w-r])
            for (y = [r, d-r])
                translate([x, y, 0])
                    cylinder(r=r, h=h);
    }
}

// =============================================
// PART 1: STAND + CRADLE (open back)
// =============================================
module stand() {
    difference() {
        union() {
            // --- BASE ---
            base();
            
            // --- COLUMN ---
            translate([base_w/2, base_d/2, base_thick])
                column();
            
            // --- CRADLE ---
            translate([base_w/2, base_d/2, base_thick + col_h])
            rotate([-tilt_angle, 0, 0])
                cradle();
        }
        
        // Cable channel down back of column
        translate([base_w/2 - cable_w/2, base_d/2 + col_d/2 - 3, base_thick])
            cube([cable_w, 5, col_h + 30]);
    }
}

module base() {
    difference() {
        rounded_rect(base_w, base_d, base_thick, base_r);
        // Weight pockets
        translate([13, base_d/2, base_thick - weight_depth])
            cylinder(d=weight_d, h=weight_depth + 0.1);
        translate([base_w - 13, base_d/2, base_thick - weight_depth])
            cylinder(d=weight_d, h=weight_depth + 0.1);
        // Grip grooves
        for (i = [0:4])
            translate([10 + i*12, 8, -0.1])
                cube([1.5, base_d - 16, 0.8]);
    }
    // Branding
    translate([base_w/2, 6, base_thick])
        linear_extrude(0.5)
            text("QuailSync", size=5, halign="center",
                 font="Liberation Sans:style=Bold");
}

module column() {
    hull() {
        // Bottom - wider for stability
        translate([-col_w/2 - 3, -col_d/2 - 2, 0])
            rounded_rect(col_w + 6, col_d + 4, 1, 3);
        // Top - matches cradle width
        translate([-col_w/2, -col_d/2, col_h - 1])
            cube([col_w, col_d, 1]);
    }
}

module cradle() {
    outer_w = pocket_w + wall*2;
    outer_h = pocket_h + floor_t;
    outer_d = pocket_d + wall;  // front wall only, back is open
    
    difference() {
        union() {
            // Main cradle body - 3 sided (front, left, right) + floor
            // Front wall
            translate([-outer_w/2, -outer_d/2, 0])
                cube([outer_w, wall, outer_h]);
            
            // Left wall
            translate([-outer_w/2, -outer_d/2, 0])
                cube([wall, outer_d, outer_h]);
            
            // Right wall
            translate([outer_w/2 - wall, -outer_d/2, 0])
                cube([wall, outer_d, outer_h]);
            
            // Floor
            translate([-outer_w/2, -outer_d/2, 0])
                cube([outer_w, outer_d, floor_t]);
            
            // Hook clips on left wall (back edge) - camera retention
            translate([-outer_w/2 - hook_thick, outer_d/2 - 0.1, floor_t + 3])
                hook_clip();
            translate([-outer_w/2 - hook_thick, outer_d/2 - 0.1, floor_t + pocket_h - 8])
                hook_clip();
            
            // Hook clips on right wall (back edge)
            translate([outer_w/2 + hook_thick, outer_d/2 - 0.1, floor_t + 3])
                mirror([1,0,0]) hook_clip();
            translate([outer_w/2 + hook_thick, outer_d/2 - 0.1, floor_t + pocket_h - 8])
                mirror([1,0,0]) hook_clip();
            
            // Grip ribs inside pocket (left wall)
            translate([-pocket_w/2 + 0.1, -pocket_d/2 + 2, floor_t + 4])
                grip_rib_vertical();
            translate([-pocket_w/2 + 0.1, -pocket_d/2 + 2, floor_t + pocket_h - 8])
                grip_rib_vertical();
            
            // Grip ribs inside pocket (right wall)
            translate([pocket_w/2 - 0.1 - rib_h, -pocket_d/2 + 2, floor_t + 4])
                grip_rib_vertical();
            translate([pocket_w/2 - 0.1 - rib_h, -pocket_d/2 + 2, floor_t + pocket_h - 8])
                grip_rib_vertical();
            
            // Grip ribs on floor
            translate([-pocket_w/4, -pocket_d/2 + 2, floor_t - 0.1])
                grip_rib_horizontal();
            translate([pocket_w/4 - rib_w, -pocket_d/2 + 2, floor_t - 0.1])
                grip_rib_horizontal();
        }
        
        // Lens hole through front wall
        translate([0, -outer_d/2 - 0.1, floor_t + pocket_h/2])
            rotate([-90, 0, 0])
                cylinder(d=cam_lens_d + 2, h=wall + 1);
        
        // Slight chamfer on back edges for easier camera insertion
        translate([-pocket_w/2 - 0.5, outer_d/2 - 0.1, floor_t - 0.1])
            rotate([0, 0, 0])
                translate([0, 0, 0])
                    cube([pocket_w + 1, 1, 0.8]);
    }
}

module hook_clip() {
    // L-shaped hook that the back plate slots under
    // Hook extends inward to catch the back plate edge
    cube([hook_thick, hook_w, hook_thick]);
    translate([0, 0, 0])
        cube([hook_overhang + hook_thick, hook_w, hook_thick]);
}

module grip_rib_vertical() {
    // Small vertical rib inside pocket wall
    cube([rib_h, pocket_d - 4, rib_w]);
}

module grip_rib_horizontal() {
    // Small horizontal rib on pocket floor
    cube([rib_w, pocket_d - 4, rib_h]);
}

// =============================================
// PART 2: BACK PLATE
// =============================================
module back_plate() {
    outer_w = pocket_w + wall*2;
    outer_h = pocket_h + floor_t;
    
    // The back plate slides down from the top
    // Hooks on the cradle catch the plate edges
    
    plate_w = outer_w + hook_thick*2 + hook_gap*2;
    plate_h = outer_h;
    
    difference() {
        union() {
            // Main plate
            translate([-plate_w/2, 0, 0])
                cube([plate_w, bp_thick, plate_h]);
            
            // Bottom locating tab (slides into gap under cradle floor)
            translate([-pocket_w/2, -bp_tab_h, 0])
                cube([pocket_w, bp_tab_h, floor_t - 0.3]);
            
            // Pull tab at top for easy removal
            translate([-8, bp_thick, plate_h - 3])
                cube([16, 6, 3]);
            // Rounded grip on pull tab
            translate([0, bp_thick + 6, plate_h - 1.5])
                rotate([0, 90, 0])
                    cylinder(d=3, h=16, center=true);
        }
        
        // Ribbon cable slot (centered, near bottom)
        translate([-cable_w/2, -0.1, floor_t + 1])
            cube([cable_w, bp_thick + 1, cable_h + 1]);
        
        // Second cable slot option (near top)
        translate([-cable_w/2, -0.1, plate_h - cable_h - 2])
            cube([cable_w, bp_thick + 1, cable_h + 1]);
        
        // Hook slots on each side - where cradle hooks catch
        // Left side slots
        translate([-plate_w/2 - 0.1, -hook_overhang, floor_t + 3])
            cube([hook_thick + hook_gap + 0.1, hook_overhang + bp_thick + 0.1, hook_w + hook_gap]);
        translate([-plate_w/2 - 0.1, -hook_overhang, floor_t + pocket_h - 8])
            cube([hook_thick + hook_gap + 0.1, hook_overhang + bp_thick + 0.1, hook_w + hook_gap]);
        
        // Right side slots  
        translate([plate_w/2 - hook_thick - hook_gap, -hook_overhang, floor_t + 3])
            cube([hook_thick + hook_gap + 0.1, hook_overhang + bp_thick + 0.1, hook_w + hook_gap]);
        translate([plate_w/2 - hook_thick - hook_gap, -hook_overhang, floor_t + pocket_h - 8])
            cube([hook_thick + hook_gap + 0.1, hook_overhang + bp_thick + 0.1, hook_w + hook_gap]);
        
        // Vent holes
        for (dx = [-5, 0, 5])
            for (dz = [plate_h * 0.3, plate_h * 0.6])
                translate([dx, -0.1, dz])
                    rotate([-90, 0, 0])
                        cylinder(d=2.5, h=bp_thick + 1);
    }
}

// =============================================
// RENDER - both parts
// =============================================

// Part 1: Stand (print as-is, base on bed)
stand();

// Part 2: Back plate (laid flat for printing)
// Shown next to stand for reference
translate([base_w/2 + 30, base_d/2, 0])
    rotate([90, 0, 0])  // lay flat for printing
        back_plate();
