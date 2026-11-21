"""Fast 4K60 construction renders from the same native meshes and edit as Blender.

Needs NumPy, ModernGL with EGL, and FFmpeg with libx264 and drawtext.
"""
import argparse
import hashlib
import re
import runpy
import subprocess
from pathlib import Path

import moderngl
import numpy as np

SOURCE = runpy.run_path(str(Path(__file__).with_name("render-construction.py")))
WIDTH, HEIGHT, FPS = 3840, 2160, 60

VERTEX = """
#version 330
in vec3 position;
in vec3 normal;
in vec2 uv;
in vec3 colour;
uniform mat3 axes;
uniform vec3 centre;
uniform vec2 half_extent;
uniform mat3 light_axes;
uniform vec3 light_centre;
uniform float light_extent;
out vec3 n;
out vec2 surface_uv;
out vec3 base;
out vec3 shadow_uv;
void main() {
    vec3 local = axes * (position - centre);
    gl_Position = vec4(local.xy / half_extent, -local.z / (8.0 * half_extent.x), 1.0);
    n = normal;
    surface_uv = uv;
    base = colour;
    vec3 lit = light_axes * (position - light_centre) / light_extent;
    shadow_uv = vec3(lit.xy, -lit.z) * 0.5 + 0.5;
}
"""

FRAGMENT = """
#version 330
uniform sampler2D cloth_image;
uniform sampler2DShadow shadow_image;
uniform float fabric_pitch;
uniform float fabric_mix;
uniform bool shadows;
uniform bool shadow_only;
in vec3 n;
in vec2 surface_uv;
in vec3 base;
in vec3 shadow_uv;
out vec4 pixel;
void main() {
    vec3 normal = normalize(n);
    if (!gl_FrontFacing) normal = -normal;
    vec3 key = normalize(vec3(-0.47, -0.61, 0.74));
    vec3 rim = normalize(vec3(0.45, 0.70, 0.25));
    float visibility = 1.0;
    if (shadows) {
        vec2 pixel_step = 1.0 / vec2(textureSize(shadow_image, 0));
        float z = shadow_uv.z - 0.0014;
        visibility = (texture(shadow_image, vec3(shadow_uv.xy + pixel_step * vec2(-1.4,-1.4), z))
                    + texture(shadow_image, vec3(shadow_uv.xy + pixel_step * vec2( 1.4,-1.4), z))
                    + texture(shadow_image, vec3(shadow_uv.xy + pixel_step * vec2(-1.4, 1.4), z))
                    + texture(shadow_image, vec3(shadow_uv.xy + pixel_step * vec2( 1.4, 1.4), z))) * 0.25;
    }
    if (shadow_only) {
        pixel = vec4(0.11, 0.085, 0.066, 0.48 * (1.0 - visibility));
        return;
    }
    float light = mix(0.29, 0.51, normal.z * 0.5 + 0.5)
                + 0.71 * visibility * max(dot(normal, key), 0.0) + 0.26 * max(dot(normal, rim), 0.0);
    vec3 colour = base;
    if (fabric_mix > 0.0) {
        vec3 swatch = pow(texture(cloth_image, surface_uv / fabric_pitch).rgb, vec3(2.2));
        colour = fabric_mix >= 1.0 ? swatch : mix(base, base * swatch, fabric_mix);
    }
    vec3 lit = colour * light + vec3(0.013, 0.010, 0.007);
    pixel = vec4(pow(clamp(lit, 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
}
"""

DEPTH_VERTEX = """
#version 330
in vec3 position;
uniform mat3 light_axes;
uniform vec3 light_centre;
uniform float light_extent;
void main() {
    vec3 p = light_axes * (position - light_centre) / light_extent;
    gl_Position = vec4(p.xy, -p.z, 1.0);
}
"""

BACKGROUND_VERTEX = """
#version 330
out vec2 position;
void main() {
    vec2 corners[3] = vec2[3](vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    position = corners[gl_VertexID];
    gl_Position = vec4(position,0.0,1.0);
}
"""

BACKGROUND_FRAGMENT = """
#version 330
in vec2 position;
out vec4 pixel;
void main() {
    vec2 p = position - vec2(-0.28, 0.36);
    float light = exp(-dot(p,p) * 0.65);
    vec3 dark = vec3(0.163, 0.147, 0.127);
    vec3 warm = vec3(0.370, 0.329, 0.268);
    pixel = vec4(mix(dark,warm,light),1.0);
}
"""

MARK_VERTEX = """
#version 330
in vec3 position;
uniform mat3 axes;
uniform vec3 centre;
uniform vec2 half_extent;
void main() {
    vec3 local = axes * (position - centre);
    gl_Position = vec4(local.xy / half_extent, -local.z / (8.0 * half_extent.x) - 0.0001, 1.0);
}
"""

MARK_FRAGMENT = """
#version 330
uniform vec3 colour;
uniform bool round_point;
out vec4 pixel;
void main() {
    if (round_point && length(gl_PointCoord - vec2(0.5)) > 0.48) discard;
    pixel = vec4(colour, 1.0);
}
"""

class Surface:
    def __init__(self, ctx, program, positions, faces, uv, colours, depth_shader=None):
        self.faces = faces.astype(np.int32)
        self.vertices = np.column_stack((positions, np.zeros_like(positions), uv, colours)).astype("f")
        self.buffer = ctx.buffer(reserve=self.vertices.nbytes, dynamic=True)
        self.indices = ctx.buffer(self.faces.tobytes())
        self.vao = ctx.vertex_array(program, [(self.buffer, "3f 3f 2f 3f", "position", "normal", "uv", "colour")],
                                    self.indices)
        self.depth = (ctx.vertex_array(depth_shader, [(self.buffer, "3f 32x", "position")], self.indices)
                      if depth_shader is not None else None)
        self.update(positions)

    def update(self, positions):
        self.vertices[:, :3] = positions
        points = positions[self.faces]
        cross = np.cross(points[:, 1] - points[:, 0], points[:, 2] - points[:, 0])
        normals = self.vertices[:, 3:6]
        normals.fill(0)
        for vertex in self.faces.T:
            np.add.at(normals, vertex, cross)
        self.buffer.write(self.vertices.tobytes())

    def colour(self, colours):
        self.vertices[:, 8:] = colours


def view(points, i, resting=False, crown=False):
    ease = SOURCE["ease"]
    yaw = .34 * ease((i-13)/57) + .32 * ease((i-78)/17)
    lift = .065 + .07*ease((i-20)/50)
    if resting and crown:
        # Lift above the brim to show how the separately cut tip meets the rolled crown.
        lift += .85*ease((i-61)/34)
    facing = np.array([np.sin(yaw), -np.cos(yaw), lift])
    facing /= np.linalg.norm(facing)
    right = np.cross([0, 0, 1], facing)
    right /= np.linalg.norm(right)
    axes = np.stack((right, np.cross(facing, right), facing))
    projected = points @ axes.T
    lower, upper = projected.min(axis=0), projected.max(axis=0)
    centre = (lower + upper) / 2 @ axes
    size = upper - lower
    width = max(size[0]*1.19, size[1]*WIDTH/HEIGHT*(1.17 if resting else 1.34))
    return axes, centre, (width/2, width/2*HEIGHT/WIDTH)


def orthogonal_light(points):
    toward = np.array([-.47, -.61, .74])
    toward /= np.linalg.norm(toward)
    right = np.cross([0, 0, 1], toward)
    right /= np.linalg.norm(right)
    basis = np.stack((right, np.cross(toward, right), toward))
    low, high = points.min(axis=0), points.max(axis=0)
    return basis, (low + high) / 2, max(0.4, np.linalg.norm(high - low) * .75)


def fabric_texture(ctx, garment):
    maps = {"sundress": ("madder", .30), "eased": ("madder", .30),
            "flare": ("madder", .30), "column": ("glen-check", .16)}
    name, pitch = maps.get(garment, (None, .19))
    image = f"atelier/{name}.webp" if name else "materials/linen.webp"
    rgb = subprocess.check_output(["ffmpeg", "-nostdin", "-v", "error", "-threads", "1", "-filter_threads", "1",
        "-i", str(SOURCE["PUBLIC"] / image), "-vf", "scale=1024:1024,vflip", "-frames:v", "1",
        "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"])
    tex = ctx.texture((1024, 1024), 3, rgb)
    tex.build_mipmaps()
    return tex, pitch, (1.0 if name else .60)


def turned_stand(center, floor, hip, radius):
    ring = np.linspace(0, 2*np.pi, 65)[:-1]
    profile = [(floor, radius), (floor+.012, radius), (floor+.020, radius*.76),
        (floor+.026, radius*.24), (hip-.023, radius*.24), (hip-.018, radius*.97), (hip, radius)]
    xyz = np.concatenate([np.column_stack((center[0]+r*np.cos(ring), center[1]+r*np.sin(ring),
        np.full(len(ring), z))) for z, r in profile])
    n = len(ring)
    faces = []
    for row in range(len(profile)-1):
        for col in range(n):
            a, b = row*n+col, row*n+(col+1)%n
            faces.extend(((a,b,a+n), (b,b+n,a+n)))
    return xyz, np.array(faces)


def trim_top(points, faces, height):
    """Cut the presentation form at a horizontal plane and cover its exposed opening."""
    below = points[:, 2] < height
    kept = below[faces]
    triangles = faces[kept.all(axis=1)].tolist()
    vertices, edge_points = [], {}
    for face in faces[kept.any(axis=1) & ~kept.all(axis=1)]:
        polygon = []
        for a, b in zip(face, np.roll(face, -1)):
            if below[a]:
                polygon.append(a)
            if below[a] != below[b]:
                key = (min(a,b), max(a,b))
                if key not in edge_points:
                    t = (height-points[a, 2])/(points[b, 2]-points[a, 2])
                    edge_points[key] = len(points) + len(vertices)
                    vertices.append(points[a] + (points[b]-points[a])*t)
                polygon.append(edge_points[key])
        triangles.extend((polygon[0], polygon[i], polygon[i+1]) for i in range(1,len(polygon)-1))
    if not vertices:
        return points, np.array(triangles)
    # Cap the central torso loop. This leaves any arms crossing the display plane separate.
    ring = np.asarray(vertices)
    width = max(0.04, np.ptp(points[:,0])*.21)
    centre_ring = np.flatnonzero(np.abs(ring[:,0]) < width)
    if len(centre_ring) >= 3:
        centre = ring[centre_ring].mean(axis=0)
        offsets = ring[centre_ring] - centre
        order = centre_ring[np.argsort(np.arctan2(offsets[:,1], offsets[:,0]))] + len(points)
        cap_id = len(points) + len(vertices)
        vertices.append(centre)
        triangles.extend((cap_id,a,b) for a,b in zip(order,np.roll(order,-1)))
    return np.vstack((points, vertices)), np.array(triangles)


def main_component(faces, point_count):
    """After cropping below the shoulders, keep the body and omit disconnected hands and arms."""
    parents = list(range(point_count))
    def root(point):
        while point != parents[point]:
            parents[point] = parents[parents[point]]
            point = parents[point]
        return point
    for a, b, c in faces:
        first = root(a)
        parents[root(b)] = first
        parents[root(c)] = first
    labels = np.fromiter((root(face[0]) for face in faces), dtype=np.int32, count=len(faces))
    return faces[labels == np.bincount(labels).argmax()]


def attached_points(points, triangles, anchors, offset):
    faces = points[triangles[anchors[:, 0].astype(int)]]
    normal = np.cross(faces[:, 1] - faces[:, 0], faces[:, 2] - faces[:, 0])
    normal /= np.maximum(np.linalg.norm(normal, axis=1, keepdims=True), 1e-12)
    weight = anchors[:, 1:]
    return (faces[:, 0] * weight[:, :1] + faces[:, 1] * weight[:, 1:]
            + faces[:, 2] * (1 - weight.sum(axis=1, keepdims=True)) + normal * offset)


def projection(program, axes, centre, extent):
    program["axes"].write(axes.T.astype("f").tobytes())
    program["centre"].value = tuple(centre)
    program["half_extent"].value = extent


def native_source(args, layout):
    if args.garment == "fez":
        return "flat pattern " + hashlib.sha256((SOURCE["PUBLIC"] / "patterns/fez.json").read_bytes()).hexdigest()
    if args.native:
        return "fold.pc2 " + hashlib.sha256((args.native / "fold.pc2").read_bytes()).hexdigest()
    return "fold.pc2 " + layout["sourcePc2Sha256"]


def render(args):
    recorded, ids, faces, panels, body, body_faces, layout, phases = SOURCE["load"](args.garment, args.native)
    cloth_colours = np.tile(SOURCE["FABRIC_COLORS"][args.garment], (len(recorded[0]), 1))
    if args.garment == "ensemble":
        for name, (start, end) in panels.items():
            if name.startswith("trousers::"):
                cloth_colours[start:end] = (.23, .084, .047)
    pattern_colours = np.tile((.65, .46, .245), (len(recorded[0]), 1))
    for index, (start, end) in enumerate(panels.values()):
        pattern_colours[start:end] *= (0.84, .91, 1.0)[index % 3]

    ctx = moderngl.create_standalone_context(backend="egl")
    target = ctx.simple_framebuffer((WIDTH, HEIGHT), components=3)
    target.use()
    background = ctx.program(vertex_shader=BACKGROUND_VERTEX, fragment_shader=BACKGROUND_FRAGMENT)
    backdrop = ctx.vertex_array(background, [])
    shading = ctx.program(vertex_shader=VERTEX, fragment_shader=FRAGMENT)
    depth = ctx.program(vertex_shader=DEPTH_VERTEX, fragment_shader="#version 330\nvoid main() {}")
    shadow_texture = ctx.depth_texture((1536, 1536))
    shadow_texture.repeat_x = shadow_texture.repeat_y = False
    shadow_texture.compare_func = "<="
    shadow_texture.filter = moderngl.LINEAR, moderngl.LINEAR
    shadow_map = ctx.framebuffer(depth_attachment=shadow_texture)
    swatch, pitch, cloth_mix = fabric_texture(ctx, args.garment)
    swatch.use(location=0)
    shadow_texture.use(location=1)
    shading["cloth_image"].value = 0
    shading["shadow_image"].value = 1
    shading["fabric_pitch"].value = pitch
    cloth = Surface(ctx, shading, recorded[0], faces, recorded[0][:, [0, 2]], pattern_colours, depth)
    marks = ctx.program(vertex_shader=MARK_VERTEX, fragment_shader=MARK_FRAGMENT)
    mark_buffer = ctx.buffer(reserve=max(1, len(faces))*6*12, dynamic=True)
    marks_vao = ctx.vertex_array(marks, [(mark_buffer, "3f", "position")])
    edges = np.sort(np.vstack([faces[:, [0, 1]], faces[:, [1, 2]], faces[:, [2, 0]]]), axis=1)
    unique, counts = np.unique(edges, axis=0, return_counts=True)
    boundary = unique[counts == 1]
    cords = [np.array(item["anchors"]).reshape(-1, 3) for item in layout.get("cords", [])]
    buttons = np.array([item["anchor"] for item in layout.get("buttons", [])])
    brass = (.82, .64, .29) if args.garment == "blue" else (.82, .81, .74)
    joins = []
    for pair in layout.get("seams", {}).values():
        left, right = np.asarray(pair["side_a"]), np.asarray(pair["side_b"])
        if min(len(left), len(right)) < 6:
            continue
        if np.linalg.norm(recorded[-1, left[0]] - recorded[-1, right[-1]]) < np.linalg.norm(recorded[-1, left[0]] - recorded[-1, right[0]]):
            right = right[::-1]
        for t in (.25, .7):
            joins.append((left[round(t*(len(left)-1))], right[round(t*(len(right)-1))]))

    def mark(points, colour, mode=moderngl.LINES, width=3):
        points = points.reshape(-1, 3).astype("f")
        mark_buffer.write(points.tobytes())
        marks["colour"].value = colour
        marks["round_point"].value = mode == moderngl.POINTS
        if mode == moderngl.POINTS:
            ctx.point_size = width
        else:
            ctx.line_width = width
        marks_vao.render(mode=mode, vertices=len(points))

    models, bounds = [], [recorded[-1]]
    floor = recorded[-1, :, 2].min() - .001
    mount = None
    if body is not None:
        floor = body[:, 2].min() - .009
        head_fraction = .18 if args.garment in ("bolero", "ensemble") else .114
        head = body[:, 2].max() - np.ptp(body[:, 2])*head_fraction
        lower_body = args.garment in ("column", "flare", "trousers")
        if lower_body:
            head = min(head, recorded[-1, :, 2].max() + .14)
        visible = np.ones(len(body), dtype=bool)
        if args.garment in ("blue", "yellow", "bolero"):
            hem = min(recorded[-1][start:end, 2].min() for piece, (start, end) in panels.items() if "back" in piece)
            hip = hem-.021
            half_width = .27 if args.garment != "bolero" else .125
            trunk = np.abs(body[:, 0]) < half_width
            drop = np.ptp(recorded[-1, :, 2]) * .28
            if args.garment == "bolero":
                visible &= (body[:, 2] >= hip) | ~trunk
            else:
                floor = min(hip-drop, recorded[-1, :, 2].min()-.022, body[~trunk, 2].min()-.025)
                visible &= (body[:, 2] >= hip) | (~trunk & (body[:, 2] >= floor))
            ring = body[trunk & (np.abs(body[:, 2]-hip)<.016)]
            low, high = ring.min(axis=0), ring.max(axis=0)
            mount = (hip, drop, (low+high)/2, (high[0]-low[0])*.48)
        body_faces = body_faces[visible[body_faces].all(axis=1)]
        body, body_faces = trim_top(body, body_faces, head)
        if lower_body or args.garment == "bolero":
            body_faces = main_component(body_faces, len(body))
        dressed_form = body[np.unique(body_faces)]
        bounds.append(dressed_form)
        models.append(Surface(ctx, shading, body, body_faces, body[:, [0, 2]], np.tile((.46,.34,.235), (len(body),1)), depth))
        if mount:
            hip, drop, center, radius = mount
            if args.garment == "bolero":
                floor = min(hip-drop, dressed_form[:, 2].min()-.027)
            stand, triangles = turned_stand(center, floor, hip, radius)
            models.append(Surface(ctx, shading, stand, triangles, stand[:, [0, 2]], np.tile((.093, .052, .030), (len(stand), 1)), depth))
            bounds.append(stand)
    bounds = np.concatenate(bounds)
    ground = np.asarray([(-20,-20,floor), (20,-20,floor), (20,20,floor), (-20,20,floor)])
    platform = Surface(ctx, shading, ground, np.array([(0,1,2),(0,2,3)]), ground[:, :2],
                       np.tile((.15,.124,.092), (4,1)))
    light_axes, light_centre, light_extent = orthogonal_light(bounds)
    for program in (shading, depth):
        program["light_axes"].write(light_axes.T.astype("f").tobytes())
        program["light_centre"].value = tuple(light_centre)
        program["light_extent"].value = light_extent

    initial_centre = (recorded[-1, :, 2].min() + recorded[-1, :, 2].max()) / 2
    gap = .045*np.ptp(recorded[-1],axis=0).max()
    board, _ = SOURCE["film_pose"](0,args.garment,recorded,ids,phases,panels,gap,initial_centre)
    axes, centre, extent = view(board, 0)
    labels = []
    for name, (start, end) in panels.items():
        panel = (board[start:end] - centre) @ axes.T
        mid = (panel.min(axis=0) + panel.max(axis=0))/2
        x, y = int((mid[0]/extent[0]+1)*WIDTH/2), int((1-mid[1]/extent[1])*HEIGHT/2)
        label = re.sub(r"[^A-Za-z0-9 ]+", " ", name.split("::")[-1].replace("dolman_", "")).strip().upper()
        font = min(40, max(16, round(np.ptp(panel[:, 0])/extent[0]*WIDTH/max(9,len(label)*2.7))))
        labels.append(f"drawtext=font=monospace:text={label}:fontsize={font}:fontcolor=0x30251d:x={x}-tw/2:y={y}-th/2:enable='lt(t,0.75)'")
    visual_phase = (
        "drawtext=font=monospace:text=FLAT PATTERN:fontsize=44:fontcolor=0xdfd2ae:x=160:y=h-150:enable='lt(t,3.3125)',"
        "drawtext=font=monospace:text=MATCH THE EDGES:fontsize=44:fontcolor=0xdfd2ae:x=160:y=h-150:enable='between(t,3.3125,3.8125)',"
        "drawtext=font=monospace:text=SEWN GARMENT:fontsize=44:fontcolor=0xdfd2ae:x=160:y=h-150:enable='gt(t,3.8125)'"
    )
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with subprocess.Popen([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-n", "-filter_threads", "2", "-f", "rawvideo",
        "-pixel_format", "rgb24", "-video_size", f"{WIDTH}x{HEIGHT}", "-framerate", str(FPS), "-i", "pipe:0",
        "-vf", ",".join(["vflip", visual_phase, *labels]) if not args.still else "vflip", "-an",
        *(["-frames:v", "1", "-threads", "1", "-update", "1"] if args.still else [
            "-c:v", "libx264", "-threads", "4", "-preset", "veryfast", "-crf", "17", "-maxrate", "9M",
            "-bufsize", "9M", "-profile:v", "high", "-level:v", "5.2", "-pix_fmt", "yuv420p",
            "-movflags", "+faststart", "-metadata", "comment=FrankenTextiles " + native_source(args, layout),
        ]), str(args.out),
    ], stdin=subprocess.PIPE) as encoder:
        initial = None
        for frame in ([args.still_frame] if args.still else range(6*FPS)):
            i = min(95, frame*16/FPS)
            if i <= 12 and initial is not None:
                pixels = initial
            else:
                points, staging = SOURCE["film_pose"](i,args.garment,recorded,ids,phases,panels,gap,initial_centre)
                axes, centre, extent = view(points if staging else bounds, i, not staging, crown=args.garment == "fez")
                cloth.colour(pattern_colours if i <= 12 else cloth_colours)
                cloth.update(points)
                if not staging:
                    shadow_map.use()
                    ctx.enable(moderngl.DEPTH_TEST)
                    shadow_map.clear(depth=1)
                    for model in models:
                        model.depth.render()
                    cloth.depth.render()
                target.use()
                target.clear(depth=1)
                ctx.disable(moderngl.DEPTH_TEST)
                backdrop.render(vertices=3)
                ctx.enable(moderngl.DEPTH_TEST)
                projection(shading, axes, centre, extent)
                projection(marks, axes, centre, extent)
                shading["shadows"].value = not staging
                shading["fabric_mix"].value = 0
                if not staging:
                    shading["shadow_only"].value = True
                    ctx.enable(moderngl.BLEND)
                    ctx.blend_func = moderngl.SRC_ALPHA, moderngl.ONE_MINUS_SRC_ALPHA
                    platform.vao.render()
                    ctx.disable(moderngl.BLEND)
                    shading["shadow_only"].value = False
                    for model in models:
                        model.vao.render()
                shading["fabric_mix"].value = 0 if i <= 12 else cloth_mix
                cloth.vao.render()
                if i <= 12:
                    mark(points[boundary], (.24, .18, .12), width=2)
                if joins and 46 <= i <= 68:
                    mark(points[joins], (.90, .65, .28), width=4)
                if not staging:
                    for cord in cords:
                        mark(attached_points(points, faces, cord, .0016), brass, moderngl.LINE_STRIP, 5)
                    if len(buttons):
                        diameter = max(4., .004 / extent[0] * WIDTH)
                        mark(attached_points(points, faces, buttons, .003), brass, moderngl.POINTS, diameter)
                pixels = target.read(components=3, alignment=1)
                if i <= 12:
                    initial = pixels
            encoder.stdin.write(pixels)
        encoder.stdin.close()
        if encoder.wait() != 0:
            raise RuntimeError("FFmpeg could not write the construction render")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("garment",choices=SOURCE["FABRIC_COLORS"])
    parser.add_argument("out", type=Path)
    parser.add_argument("native", type=Path, nargs="?")
    parser.add_argument("--still", action="store_true")
    parser.add_argument("--still-frame", type=int, default=333)
    args = parser.parse_args()
    if not 0 <= args.still_frame < 6*FPS:
        parser.error("--still-frame must be within 0 through 359")
    if args.out.suffix.lower() != (".png" if args.still else ".mp4"):
        parser.error("choose a new .png path for a still or .mp4 path for a film")
    if args.out.exists():
        parser.error(f"{args.out} already exists")
    if args.native is None and args.garment in ("bolero","trousers","column","flare","ensemble"):
        parser.error("this garment needs its original native drape directory")
    render(args)
