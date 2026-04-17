#!/usr/bin/env python3
"""Generate Earth-like SVG paths for MME map territories.

Downloads Natural Earth 110m country boundaries, maps them to MME territories,
projects to the MME coordinate system, and generates SVG path strings.
"""

import json
import math
import urllib.request
import numpy as np
from shapely.geometry import shape, Polygon, MultiPolygon, box
from shapely.ops import unary_union
from scipy.spatial import Voronoi

# Natural Earth 110m countries GeoJSON URL
GEOJSON_URL = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_admin_0_countries.geojson"

# MME territory -> list of Natural Earth country names
# For countries split into multiple territories, we list the parent country
# and subdivide using Voronoi based on centroid positions
TERRITORY_COUNTRY_MAP = {
    # Eastern US (0-4)
    0: ["United States of America"],  # New England
    1: ["United States of America"],  # New York
    2: ["United States of America"],  # Appalachia
    3: ["United States of America"],  # Southeast US
    4: ["United States of America"],  # Midwest
    # Western US (5-8)
    5: ["United States of America"],  # Great Plains
    6: ["United States of America"],  # Rocky Mountains
    7: ["United States of America"],  # Texas
    8: ["United States of America"],  # Pacific States
    # Canada (9-13)
    9: ["Canada"],   # Quebec
    10: ["Canada"],  # Ontario
    11: ["Canada"],  # Prairies
    12: ["Canada"],  # British Columbia
    13: ["Canada"],  # Northern Territories
    # Central America (14-16)
    14: ["Mexico"],
    15: ["Guatemala", "Belize", "Honduras", "El Salvador", "Nicaragua", "Costa Rica", "Panama"],
    16: ["Cuba", "Jamaica", "Haiti", "Dominican Rep.", "Puerto Rico", "Trinidad and Tobago", "Bahamas"],
    # Northern SA (17-20)
    17: ["Venezuela"],
    18: ["Colombia", "Ecuador"],
    19: ["Guyana", "Suriname", "Fr. S. Antarctic Lands"],
    20: ["Brazil"],
    # Southern SA (21-23)
    21: ["Peru"],
    22: ["Bolivia", "Paraguay"],
    23: ["Argentina", "Chile", "Uruguay", "Falkland Is."],
    # West Europe (24-27)
    24: ["United Kingdom", "Ireland"],
    25: ["France"],
    26: ["Belgium", "Netherlands", "Luxembourg"],
    27: ["Germany", "Austria", "Switzerland", "Czechia"],
    # South Europe (28-31)
    28: ["Spain", "Portugal"],
    29: ["Italy"],
    30: ["Greece", "Cyprus"],
    31: ["Croatia", "Bosnia and Herz.", "Serbia", "Montenegro", "Kosovo", "North Macedonia", "Albania", "Bulgaria", "Slovenia"],
    # Scandinavia (32-34)
    32: ["Norway"],
    33: ["Sweden", "Denmark"],
    34: ["Finland", "Estonia", "Latvia", "Lithuania"],
    # East Europe (35-38)
    35: ["Poland"],
    36: ["Hungary", "Slovakia", "Romania"],
    37: ["Romania", "Moldova"],
    38: ["Ukraine", "Belarus"],
    # West Russia (39-43)
    39: ["Russia"],  # Moscow
    40: ["Russia"],  # Southern Russia
    41: ["Russia"],  # Ural
    42: ["Russia"],  # Volga
    43: ["Kazakhstan", "Uzbekistan", "Turkmenistan", "Kyrgyzstan", "Tajikistan"],
    # East Russia (44-49)
    44: ["Russia"],  # West Siberia
    45: ["Russia"],  # Central Siberia
    46: ["Russia"],  # South Siberia
    47: ["Russia"],  # Yakutsk
    48: ["Russia"],  # Magadan
    49: ["Russia"],  # Kamchatka
    # North Africa (50-53)
    50: ["Morocco", "W. Sahara"],
    51: ["Algeria", "Tunisia"],
    52: ["Libya"],
    53: ["Egypt"],
    # West Africa (54-56)
    54: ["Senegal", "Gambia", "Guinea-Bissau", "Guinea", "Sierra Leone", "Liberia", "Mali", "Mauritania"],
    55: ["Côte d'Ivoire", "Ghana", "Togo", "Benin", "Burkina Faso"],
    56: ["Nigeria", "Cameroon", "Niger"],
    # East Africa (57-60)
    57: ["Chad", "Central African Rep."],
    58: ["Dem. Rep. Congo", "Congo", "Gabon", "Eq. Guinea"],
    59: ["Ethiopia", "Eritrea", "Djibouti", "Somalia", "Sudan", "S. Sudan"],
    60: ["Kenya", "Uganda", "Rwanda", "Burundi", "Tanzania"],
    # South Africa (61-63)
    61: ["Angola", "Zambia", "Namibia"],
    62: ["Mozambique", "Malawi", "Madagascar", "Zimbabwe"],
    63: ["South Africa", "Botswana", "Lesotho", "eSwatini"],
    # Middle East (64-67)
    64: ["Turkey"],
    65: ["Iran"],
    66: ["Iraq", "Syria", "Jordan", "Lebanon", "Israel"],
    67: ["Saudi Arabia", "Yemen", "Oman", "United Arab Emirates", "Qatar"],
    # Central Asia (68-71)
    68: ["Uzbekistan", "Tajikistan"],
    69: ["Turkmenistan"],
    70: ["Afghanistan", "Pakistan"],
    71: ["Kyrgyzstan"],
    # India (72-75)
    72: ["Pakistan"],
    73: ["India"],  # Northern India
    74: ["India"],  # Southern India
    75: ["Bangladesh", "Myanmar", "Nepal", "Bhutan"],
    # East Asia (76-80)
    76: ["Mongolia"],
    77: ["China"],  # Northern China
    78: ["China"],  # Western China
    79: ["China"],  # Southern China
    80: ["Japan", "South Korea", "North Korea", "Taiwan"],
    # Southeast Asia (81-84)
    81: ["Thailand", "Laos", "Cambodia"],
    82: ["Vietnam"],
    83: ["Malaysia", "Brunei"],
    84: ["Indonesia", "Timor-Leste", "Papua New Guinea"],
    # Oceania (85-88)
    85: ["Australia"],  # Western Australia
    86: ["Australia"],  # Northern Australia
    87: ["Australia"],  # Eastern Australia
    88: ["New Zealand"],
}

# Countries that are split into multiple MME territories
SPLIT_COUNTRIES = {
    "United States of America": [0, 1, 2, 3, 4, 5, 6, 7, 8],
    "Canada": [9, 10, 11, 12, 13],
    "Russia": [39, 40, 41, 42, 44, 45, 46, 47, 48, 49],
    "China": [77, 78, 79],
    "India": [73, 74],
    "Australia": [85, 86, 87],
}

# Countries shared between territories (appear in multiple territory mappings)
# We handle these by assigning to the first territory that lists them
SHARED_COUNTRIES = {
    "Romania": 37,  # Primary assignment
    "Uzbekistan": 68,
    "Turkmenistan": 69,
    "Kyrgyzstan": 71,
    "Pakistan": 72,
}


def fetch_geojson():
    """Fetch Natural Earth 110m countries GeoJSON."""
    cache_path = "/tmp/ne_110m_countries.geojson"
    try:
        with open(cache_path) as f:
            return json.load(f)
    except FileNotFoundError:
        pass

    print("Downloading Natural Earth 110m countries...")
    req = urllib.request.Request(GEOJSON_URL, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read())

    with open(cache_path, "w") as f:
        json.dump(data, f)

    return data


def build_country_geometries(geojson):
    """Build a dict of country name -> Shapely geometry."""
    countries = {}
    for feature in geojson["features"]:
        name = feature["properties"].get("NAME") or feature["properties"].get("name")
        if name:
            try:
                geom = shape(feature["geometry"])
                if geom.is_valid:
                    countries[name] = geom
                else:
                    countries[name] = geom.buffer(0)
            except Exception as e:
                print(f"Warning: Could not parse geometry for {name}: {e}")
    return countries


def compute_transform(mme_territories):
    """Compute affine transform from lon/lat to MME coordinates.

    Uses reference points from known territory positions.
    Returns function that transforms (lon, lat) -> (x, y).
    """
    # Reference points: (territory_id, approximate lon, lat)
    refs = [
        (8, -122, 38),    # Pacific States
        (0, -72, 43),     # New England
        (13, -100, 65),   # Northern Territories
        (14, -102, 23),   # Mexico
        (23, -65, -35),   # Argentina
        (20, -50, -12),   # Brazil
        (24, -2, 54),     # Britain
        (25, 2, 47),      # France
        (32, 10, 63),     # Norway
        (34, 26, 62),     # Finland
        (53, 30, 27),     # Egypt
        (63, 25, -30),    # South Africa
        (64, 35, 39),     # Turkey
        (65, 53, 32),     # Iran
        (39, 37, 56),     # Moscow
        (76, 100, 47),    # Mongolia
        (77, 110, 38),    # Northern China
        (80, 133, 36),    # Korea-Japan
        (49, 160, 57),    # Kamchatka
        (84, 120, -5),    # Indonesia
        (87, 150, -28),   # Eastern Australia
        (88, 172, -42),   # New Zealand
    ]

    # Extract label positions and real-world coordinates
    lons = []
    lats = []
    xs = []
    ys = []

    for tid, lon, lat in refs:
        lp = mme_territories[tid]["visual"]["label_pos"]
        lons.append(lon)
        lats.append(lat)
        xs.append(lp[0])
        ys.append(lp[1])

    lons = np.array(lons, dtype=float)
    lats = np.array(lats, dtype=float)
    xs = np.array(xs, dtype=float)
    ys = np.array(ys, dtype=float)

    # Fit affine: x = a*lon + b, y = c*lat + d (least squares)
    A_x = np.column_stack([lons, np.ones_like(lons)])
    x_params, _, _, _ = np.linalg.lstsq(A_x, xs, rcond=None)

    A_y = np.column_stack([lats, np.ones_like(lats)])
    y_params, _, _, _ = np.linalg.lstsq(A_y, ys, rcond=None)

    a, b = x_params
    c, d = y_params

    print(f"Transform: x = {a:.3f}*lon + {b:.3f}, y = {c:.3f}*lat + {d:.3f}")

    # Verify with some reference points
    for tid, lon, lat in refs[:5]:
        lp = mme_territories[tid]["visual"]["label_pos"]
        px = a * lon + b
        py = c * lat + d
        print(f"  {mme_territories[tid]['name']}: expected ({lp[0]}, {lp[1]}), got ({px:.0f}, {py:.0f})")

    def transform(lon, lat):
        return (a * lon + b, c * lat + d)

    return transform


def transform_geometry(geom, transform_fn):
    """Transform a Shapely geometry using the given function."""
    from shapely.ops import transform as shapely_transform

    def apply_transform(x, y, z=None):
        coords = np.array(list(zip(x, y)))
        result = np.array([transform_fn(lon, lat) for lon, lat in coords])
        return result[:, 0], result[:, 1]

    return shapely_transform(apply_transform, geom)


def subdivide_country(country_geom, territory_ids, centroids, transform_fn):
    """Subdivide a country geometry among multiple territories using Voronoi."""
    from scipy.spatial import Voronoi

    # Get centroids in MME space
    mme_points = np.array([centroids[tid] for tid in territory_ids])

    if len(territory_ids) == 1:
        return {territory_ids[0]: country_geom}

    # Transform country to MME space
    mme_country = transform_geometry(country_geom, transform_fn)

    # Create Voronoi with mirror points for bounded cells
    bounds = mme_country.bounds  # (minx, miny, maxx, maxy)
    margin = 500
    mirror = []
    for p in mme_points:
        mirror.append([2 * bounds[0] - margin - p[0], p[1]])
        mirror.append([2 * bounds[2] + margin - p[0], p[1]])
        mirror.append([p[0], 2 * bounds[1] - margin - p[1]])
        mirror.append([p[0], 2 * bounds[3] + margin - p[1]])

    all_points = np.vstack([mme_points, mirror])

    try:
        vor = Voronoi(all_points)
    except Exception as e:
        print(f"  Voronoi failed: {e}, falling back to equal split")
        return {tid: mme_country for tid in territory_ids}

    result = {}
    for i, tid in enumerate(territory_ids):
        region_idx = vor.point_region[i]
        region = vor.regions[region_idx]

        if -1 in region or len(region) < 3:
            # Fallback: buffer around centroid
            pt = mme_points[i]
            cell = Polygon([(pt[0]-50, pt[1]-50), (pt[0]+50, pt[1]-50),
                          (pt[0]+50, pt[1]+50), (pt[0]-50, pt[1]+50)])
        else:
            verts = [vor.vertices[v] for v in region]
            cell = Polygon(verts)

        try:
            piece = mme_country.intersection(cell)
            if piece.is_empty or piece.area < 1:
                # Fallback
                piece = mme_country.intersection(cell.buffer(20))
            result[tid] = piece
        except Exception:
            result[tid] = mme_country

    return result


def geometry_to_svg_path(geom, simplify_tolerance=1.5):
    """Convert a Shapely geometry to an SVG path string."""
    if geom.is_empty:
        return ""

    geom = geom.simplify(simplify_tolerance, preserve_topology=True)

    def polygon_to_path(poly):
        parts = []
        # Exterior ring
        coords = list(poly.exterior.coords)
        if len(coords) < 3:
            return ""
        parts.append(f"M{coords[0][0]:.1f} {coords[0][1]:.1f}")
        for x, y in coords[1:]:
            parts.append(f"L{x:.1f} {y:.1f}")
        parts.append("z")
        return " ".join(parts)

    if isinstance(geom, Polygon):
        return polygon_to_path(geom)
    elif isinstance(geom, MultiPolygon):
        paths = []
        # Sort by area, largest first
        polys = sorted(geom.geoms, key=lambda p: p.area, reverse=True)
        for poly in polys:
            p = polygon_to_path(poly)
            if p:
                paths.append(p)
        return " ".join(paths)
    else:
        # Try to extract polygons from geometry collection
        try:
            polys = [g for g in geom.geoms if isinstance(g, (Polygon, MultiPolygon))]
            if polys:
                merged = unary_union(polys)
                return geometry_to_svg_path(merged, simplify_tolerance)
        except Exception:
            pass
        return ""


def main():
    with open("maps/mme.json") as f:
        data = json.load(f)

    territories = data["territories"]
    centroids = {t["id"]: t["visual"]["label_pos"] for t in territories}

    # Fetch and parse country geometries
    geojson = fetch_geojson()
    countries = build_country_geometries(geojson)

    print(f"Loaded {len(countries)} countries")
    available = set(countries.keys())

    # Compute coordinate transform
    transform_fn = compute_transform(territories)

    # Clip box in MME space
    clip_box = box(15, 15, 960, 640)

    # First pass: handle split countries (subdivide using Voronoi)
    split_results = {}
    for country_name, tids in SPLIT_COUNTRIES.items():
        if country_name not in countries:
            print(f"Warning: Split country '{country_name}' not found in GeoJSON")
            continue

        print(f"Subdividing {country_name} into {len(tids)} territories...")
        pieces = subdivide_country(countries[country_name], tids, centroids, transform_fn)
        for tid, geom in pieces.items():
            split_results[tid] = geom

    # Second pass: process all territories
    for territory in territories:
        tid = territory["id"]
        name = territory["name"]

        if tid in split_results:
            # Already subdivided
            mme_geom = split_results[tid]
        else:
            # Merge all mapped countries for this territory
            country_names = TERRITORY_COUNTRY_MAP.get(tid, [])
            geoms = []
            for cn in country_names:
                # Skip shared countries if this isn't the primary territory
                if cn in SHARED_COUNTRIES and SHARED_COUNTRIES[cn] != tid:
                    # Check if this country is in a split country
                    if cn in SPLIT_COUNTRIES:
                        continue
                    # Include a portion near this territory's centroid
                    pass

                if cn in countries:
                    geoms.append(countries[cn])
                else:
                    # Try fuzzy match
                    for real_name in available:
                        if cn.lower() in real_name.lower() or real_name.lower() in cn.lower():
                            geoms.append(countries[real_name])
                            break
                    else:
                        print(f"  Warning: Country '{cn}' not found for territory {tid} ({name})")

            if not geoms:
                print(f"  ERROR: No geometry for territory {tid} ({name})")
                continue

            merged = unary_union(geoms)
            mme_geom = transform_geometry(merged, transform_fn)

        # Clip to viewbox
        try:
            clipped = mme_geom.intersection(clip_box)
            if clipped.is_empty:
                clipped = mme_geom
        except Exception:
            clipped = mme_geom

        # Generate SVG path
        path = geometry_to_svg_path(clipped, simplify_tolerance=1.0)

        if path:
            territory["visual"]["path"] = path
            print(f"  ✓ Territory {tid:2d} ({name}): path generated")
        else:
            print(f"  ✗ Territory {tid:2d} ({name}): EMPTY path!")

    # Write updated JSON
    with open("maps/mme.json", "w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")

    # Count results
    filled = sum(1 for t in territories if t["visual"]["path"])
    print(f"\nDone! {filled}/{len(territories)} territories have paths")


if __name__ == "__main__":
    main()
