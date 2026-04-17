#!/usr/bin/env python3
"""Generate SVG paths for MME map territories using Voronoi tessellation."""

import json
import numpy as np
from scipy.spatial import Voronoi

def clip_polygon_to_rect(polygon, xmin, ymin, xmax, ymax):
    """Sutherland-Hodgman algorithm to clip a polygon to a rectangle."""
    def clip_edge(poly, edge_func, inside_func):
        if len(poly) == 0:
            return []
        result = []
        for i in range(len(poly)):
            curr = poly[i]
            prev = poly[i - 1]
            curr_inside = inside_func(curr)
            prev_inside = inside_func(prev)
            if curr_inside:
                if not prev_inside:
                    result.append(edge_func(prev, curr))
                result.append(curr)
            elif prev_inside:
                result.append(edge_func(prev, curr))
        return result

    def intersect_left(p1, p2):
        t = (xmin - p1[0]) / (p2[0] - p1[0]) if p2[0] != p1[0] else 0
        return [xmin, p1[1] + t * (p2[1] - p1[1])]

    def intersect_right(p1, p2):
        t = (xmax - p1[0]) / (p2[0] - p1[0]) if p2[0] != p1[0] else 0
        return [xmax, p1[1] + t * (p2[1] - p1[1])]

    def intersect_bottom(p1, p2):
        t = (ymin - p1[1]) / (p2[1] - p1[1]) if p2[1] != p1[1] else 0
        return [p1[0] + t * (p2[0] - p1[0]), ymin]

    def intersect_top(p1, p2):
        t = (ymax - p1[1]) / (p2[1] - p1[1]) if p2[1] != p1[1] else 0
        return [p1[0] + t * (p2[0] - p1[0]), ymax]

    poly = list(polygon)
    poly = clip_edge(poly, intersect_left, lambda p: p[0] >= xmin)
    poly = clip_edge(poly, intersect_right, lambda p: p[0] <= xmax)
    poly = clip_edge(poly, intersect_bottom, lambda p: p[1] >= ymin)
    poly = clip_edge(poly, intersect_top, lambda p: p[1] <= ymax)
    return poly


def round_corners(points, radius=3.0):
    """Generate an SVG path with slightly rounded corners."""
    if len(points) < 3:
        return ""

    n = len(points)
    parts = []

    for i in range(n):
        p0 = np.array(points[(i - 1) % n])
        p1 = np.array(points[i])
        p2 = np.array(points[(i + 1) % n])

        # Vectors from p1 to neighbors
        v0 = p0 - p1
        v2 = p2 - p1

        d0 = np.linalg.norm(v0)
        d2 = np.linalg.norm(v2)

        if d0 < 1e-6 or d2 < 1e-6:
            continue

        # Limit radius to half the shorter edge
        r = min(radius, d0 * 0.4, d2 * 0.4)

        # Points where the curve starts/ends
        start = p1 + (v0 / d0) * r
        end = p1 + (v2 / d2) * r

        if i == 0:
            parts.append(f"M{start[0]:.1f} {start[1]:.1f}")
        else:
            parts.append(f"L{start[0]:.1f} {start[1]:.1f}")

        # Quadratic bezier through the corner
        parts.append(f"Q{p1[0]:.1f} {p1[1]:.1f} {end[0]:.1f} {end[1]:.1f}")

    parts.append("z")
    return " ".join(parts)


def main():
    with open("maps/mme.json") as f:
        data = json.load(f)

    territories = data["territories"]
    points = np.array([t["visual"]["label_pos"] for t in territories])

    # Bounding box with padding
    xmin, ymin = points.min(axis=0) - 30
    xmax, ymax = points.max(axis=0) + 30

    # Add mirror points outside bounds to ensure all cells are finite
    margin = 300
    mirror_points = []
    for p in points:
        mirror_points.append([2 * xmin - margin - p[0], p[1]])
        mirror_points.append([2 * xmax + margin - p[0], p[1]])
        mirror_points.append([p[0], 2 * ymin - margin - p[1]])
        mirror_points.append([p[0], 2 * ymax + margin - p[1]])

    all_points = np.vstack([points, mirror_points])
    vor = Voronoi(all_points)

    for i, territory in enumerate(territories):
        region_idx = vor.point_region[i]
        region = vor.regions[region_idx]

        if -1 in region or len(region) == 0:
            # Fallback: simple circle-ish shape around centroid
            cx, cy = points[i]
            r = 15
            territory["visual"]["path"] = (
                f"M{cx-r:.1f} {cy:.1f} "
                f"Q{cx-r:.1f} {cy-r:.1f} {cx:.1f} {cy-r:.1f} "
                f"Q{cx+r:.1f} {cy-r:.1f} {cx+r:.1f} {cy:.1f} "
                f"Q{cx+r:.1f} {cy+r:.1f} {cx:.1f} {cy+r:.1f} "
                f"Q{cx-r:.1f} {cy+r:.1f} {cx-r:.1f} {cy:.1f} z"
            )
            print(f"Warning: territory {i} ({territory['name']}) has unbounded Voronoi cell, using fallback")
            continue

        # Get vertices of the Voronoi cell
        vertices = [vor.vertices[v].tolist() for v in region]

        # Clip to bounding rect
        clipped = clip_polygon_to_rect(vertices, xmin, ymin, xmax, ymax)

        if len(clipped) < 3:
            cx, cy = points[i]
            r = 15
            territory["visual"]["path"] = (
                f"M{cx-r:.1f} {cy:.1f} "
                f"Q{cx-r:.1f} {cy-r:.1f} {cx:.1f} {cy-r:.1f} "
                f"Q{cx+r:.1f} {cy-r:.1f} {cx+r:.1f} {cy:.1f} "
                f"Q{cx+r:.1f} {cy+r:.1f} {cx:.1f} {cy+r:.1f} "
                f"Q{cx-r:.1f} {cy+r:.1f} {cx-r:.1f} {cy:.1f} z"
            )
            print(f"Warning: territory {i} ({territory['name']}) clipped to nothing, using fallback")
            continue

        # Sort vertices by angle from centroid for proper polygon ordering
        cx, cy = points[i]
        angles = [np.arctan2(p[1] - cy, p[0] - cx) for p in clipped]
        sorted_verts = [v for _, v in sorted(zip(angles, clipped))]

        path = round_corners(sorted_verts, radius=4.0)
        territory["visual"]["path"] = path

    # Write back
    with open("maps/mme.json", "w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")

    print(f"Generated paths for {len(territories)} territories")
    print(f"Bounding box: ({xmin:.0f}, {ymin:.0f}) to ({xmax:.0f}, {ymax:.0f})")


if __name__ == "__main__":
    main()
