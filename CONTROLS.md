# Harbor Heat — Controls

Harbor Heat is an original open-city sandbox set in the fictional coastal city of Port Solace.

## Running

| Command | What it does |
|---|---|
| `cargo run --release` | Play (optimised build, recommended) |
| `cargo dev` | Fast-iteration debug build with Bevy dynamically linked (alias for `cargo run --features dev`) |

Click inside the window to capture the mouse. Press **Esc** to release it.

## Key bindings

All keys can be remapped in **`keybinds.cfg`**, which is created next to where you run the game the first time it starts. Each line is `action = key[, key]`, and the file header lists the valid key names. Delete the file to restore the defaults.

## On foot

| Input | Action |
|---|---|
| W A S D / arrow keys | Move (relative to the camera): jog by default |
| Left Shift | Sprint |
| Left Alt (hold) | Walk |
| Space | Jump |
| Mouse | Orbit the camera |
| Right mouse | Aim: over-the-shoulder zoom, tighter spread, and you strafe facing the crosshair (the sniper rifle goes to a scope) |
| Left mouse | Fire / punch / swing / throw |
| R | Reload |
| Tab (hold) | **Weapon wheel**: time slows down, move the mouse toward a weapon, release Tab to equip it |
| 1-5 | Fists, pistol, SMG, shotgun, rocket launcher (if you have them) |
| Q / E or mouse wheel | Previous / next weapon you're carrying |
| V | Cycle camera distance |
| F or Enter | Enter the nearest vehicle (an NPC or cop driver gets dragged out) |
| Esc or P | Pause (releases the mouse) |

### Weapons

| Weapon | Notes |
|---|---|
| Fists | Always with you |
| Baseball Bat | Two-handed swing that knocks people flat |
| Pistol | Semi-auto, accurate when aimed. You start with it. |
| Revolver | Six heavy rounds, big kick, 4x headshots |
| SMG | Fast full-auto, light recoil |
| Assault Rifle | Accurate full-auto, two-handed |
| Pump Shotgun | Nine pellets, deadly up close |
| Sniper Rifle | Hold aim for the scope (13 degree FOV), 4x headshots |
| Minigun | Spins up for about 0.7 s, then 25 rounds a second; you move slowly |
| Grenades | Thrown in an arc, bounce, and explode after about 3 s |
| Rocket Launcher | Explodes on impact |

How shooting works:
- **Spread:** bullets go inside a cone that's tight when you aim and stand still, and wider from the hip, while moving or in the air. Holding the trigger widens it (bloom); it closes again within a fraction of a second.
- **Feedback:** the crosshair ticks open and close with the cone, and a hit marker flashes on a hit (red on a kill).
- **Recoil:** each shot kicks the view up and a little sideways, and most of the climb settles back on its own.
- **Hits:** bullets are re-traced from the muzzle, so cover blocks them. Damage falls off past half range, and headshots count 2-4x. Shots push cars and bodies; people flinch when hit and fall along the line of the shot when killed. You'll see blood, sparks, dust, bullet holes and ejected brass.
- **Finding weapons:** every gun can be found as a pickup or dropped by the dead (Tab opens the wheel once you have them).

Movement: jumps have a little coyote time and an input buffer, you fall faster than you rise, and long drops hurt or knock you down. Out of trouble for a few seconds, health creeps back up to half.

## Driving

| Input | Action |
|---|---|
| W | Accelerate |
| S | Brake, then reverse once stopped |
| A / D | Steer |
| Space | Handbrake (drift) |
| H | Horn |
| Q / E | Previous / next radio station (Neon Tide 104.4, Harbor Heat 88, Solace Rock 132, off) |
| J | In a taxi: start / quit the taxi job |
| C (hold) | Look behind |
| V | Cycle chase-camera distance |
| F or Enter | Exit the vehicle (works at speed, so you can bail out of a burning car) |

Cars take damage from impacts and gunfire. A badly damaged car smokes, then catches fire, then explodes about 5 seconds later.

## Flying (planes and helicopter)

| Input | Plane | Helicopter |
|---|---|---|
| W / S | Nose down / nose up | Tilt forward / back |
| A / D | Roll left / right (also nose-wheel steering on the ground) | Roll left / right |
| Q / E | Yaw (rudder) | Turn left / right |
| Left Shift / Left Ctrl | Throttle up / down | Climb / descend (collective) |
| Space | Wheel brakes | - |
| F or Enter | Get out | Get out |

- Planes need speed to fly. Lift depends on airspeed and angle of attack, and a stall drops the nose. The Skylark 172 takes off at about 130-190 km/h; the Falcon 9X jet needs more runway.
- Aircraft are parked at the airport (west), plus a helicopter on the hospital roof. They show as yellow dots on the minimap.

## Map and menus

| Key | Action |
|---|---|
| M | Full-screen map: left click sets a waypoint, right click clears it |
| Esc / P | Pause menu: Resume, Map, Spawn vehicle, Quit |
| F1 | Vehicle spawn menu: press 1-9 to drop any car, the police cruiser, a plane, the jet or the helicopter in front of you |
| F2 | Skip the clock forward 3 hours |

The minimap shows your waypoint (purple, with its distance), the current job objective (yellow), job markers (yellow courier, red contract, green export), targets (red crew, green car, blue taxi fare), Spray & Go garages (pink), nearby stunt ramps (amber), police (flashing red/blue) and aircraft (yellow). Blips stick to the edge when they are off the minimap.

## The city

- **Traffic** drives on the right and obeys the traffic lights, which alternate east-west and north-south every ~10 s. Cars stop for whatever is in front of them. Carjack a traffic car with F.
- **Pedestrians** walk the sidewalks and dive out of the way of speeding cars (not all of them make it). Hit or shoot someone and they might fight back; a few carry a pistol.
- **Day and night:** a game day passes in 24 minutes, starting at 17:00. At night the windows light up, street lamps light the pavement and your car gets headlights. F2 skips 3 hours.
- **Audio:** engine sounds pitched by revs and gear, tyre screech, gunfire, explosions, crashes, sirens, footsteps, and three in-car radio stations.

## Money, pickups and jobs

- **Pickups** spin above the pavement all over town and come back about a minute after you take them: first-aid kits (+50 health), body armour (full armour, which soaks up damage before your health does), cash, and guns with ammo. You start with just a pistol; the SMG, shotgun and rocket launcher have to be found. There's armour outside the police HQ, health outside the hospital, guns in the parks and a rocket launcher on the quay. Dead pedestrians drop cash, and armed ones drop their gun.
- **Spray & Go** (pink sign; two garages, in west midtown and east midtown): drive in and stop. The door rolls down and you come out repaired, repainted and with no wanted level, for $100.
- **Jobs:** step or drive into a glowing marker.
  - **Courier** (yellow, next to where you start): four drop-offs across town, each on a timer. $150 a drop plus a $600 bonus.
  - **Contract** (red, industrial east side): take out the four-member Rustline Crew on their corner. They're armed. $2,500.
  - **Export** (green, south midtown): steal a specific gold sports car (its alarm gets you a star) and deliver it to the docks export yard before the timer runs out. You get up to $3,000, less for damage.
  - **Taxi:** get in any cab and press J. Pick up the fare (blue blip), then drop them off before the meter runs out. The fare pays for distance plus time left over, and there's a streak bonus every 5 fares. J again quits.
  - The objective line at the bottom shows what to do and how far away it is, and the timer at the top counts down. Dying or getting busted ends the job.
- **Stunt jumps:** yellow-and-black ramps on the airport road, along the beach (two), on the airport apron (the big one) and on the north quay. Hit one fast: the jump plays in slow motion the first time, and landing it pays a stunt bonus. Any big jump also pays an airtime bonus for distance, height, flips, barrel rolls and spins, but only if you land on your wheels.

## Crime and police

- Shooting, assault, carjacking and killing raise your **wanted level** (stars, top right). Killing or shooting at cops raises it fastest.
- Police cars are dispatched by star level. In a car they ram you; on foot they pull up, and cops get out and chase you. From 2 stars they shoot.
- **Escaping:** the stars blink while no cop can see you. Stay out of sight long enough and they clear.
- **Busted:** at 1-2 stars, a cop standing next to you while you're slow on foot arrests you. You're released at the police station with a fine and lose your weapons.
- **Wasted:** you wake up at the hospital and pay a bill.
- Pedestrians wander the sidewalks and flee from gunfire and explosions. Dead bodies are physics ragdolls.

## HUD

- **Top right:** money
- **Bottom left:** minimap, rotating with the camera; **N** marks north. The health bar (green) and armour bar (blue) sit under it.
- **Bottom right:** speedometer, vehicle name and vehicle condition (only while driving)
- **F3:** toggle the FPS counter

## Graphics

| Key | Action |
|---|---|
| F4 | Switch lighting between **raster** (default) and **ray traced** |

- **Raster** (default): cascaded sun shadows, SSAO, screen-space reflections on glass and water, sky-based ambient lighting, and TAA. It runs at about 120-135 fps on an RTX 3070 with traffic and crowds.
- **Ray traced** (Bevy Solari + NVIDIA **DLSS Ray Reconstruction**): ray-traced sun shadows, global illumination, reflections and sky light, denoised by DLSS. It runs at about 52-58 fps on an RTX 3070. Needs an RTX GPU, and turns on about 2.5 s after start-up.

### Building with DLSS

The project expects:
- the NVIDIA DLSS SDK at **v310.9.1** in `./DLSS` (`git -C DLSS checkout v310.9.1`)
- the Vulkan SDK at `C:/VulkanSDK/1.4.363.1`
- clang's `libclang.dll`

These paths are set in `.cargo/config.toml`. `build.rs` copies `nvngx_dlss.dll` and `nvngx_dlssd.dll` next to the executable; ship them, with NVIDIA's license text, if you distribute the game.

## Developer / test switches (environment variables)

| Variable | Effect |
|---|---|
| `HH_AUTOTEST=1` | Scripted smoke test: walks, drives, drifts, crashes, carjacks and saves screenshots to `autotest/`, then exits and prints FPS stats |
| `HH_BENCH=1` | Fixed-camera frame-rate benchmark (combine with `HH_NOVSYNC=1`) |
| `HH_WORLDTEST=1` | Traffic, map, menus, the vehicle spawner and the full arrest (BUSTED) chain |
| `HH_FLIGHTTEST=1` | Takes off in the Skylark, then flies the helicopter |
| `HH_TOGGLETEST=1` | Switches raster -> ray traced -> raster at runtime |
| `HH_COMBATTEST=1` | Scripted combat test: shoots a pedestrian, triggers police, fires a rocket, sprays the SMG |
| `HH_GUNTEST=1` | Films walk, jog, sprint and strafe from the side; uses the weapon wheel; fires the rifle, sniper, grenade, minigun, bat and shotgun at targets; prints PASS/FAIL |
| `HH_JOBTEST=1` | Runs pickups, armour, all four job types, the respray, a stunt jump and a ped fighting back, then prints PASS/FAIL for each |
| `HH_WALKTEST=1` | Logs the character controller while sprinting in a straight line |
| `HH_RT=1` | Start in ray-traced lighting mode |
| `HH_SHOTS=<dir>` | Save fixed comparison screenshots (respray garage, pickups, job marker, stunt ramp, drivers in cars, street, cars, player, aerial, docks) to `<dir>` and exit |
| `HH_HOUR=<0-24>` | Start at that time of day |
| `HH_NO_SHADOW=1`, `HH_NO_ATMO=1` | Disable sun shadows / the atmosphere (for profiling) |
