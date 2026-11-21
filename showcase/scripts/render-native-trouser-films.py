"""Render the published trouser surfaces and sectional diagrams at 4K60 with EGL and FFmpeg."""
import argparse
import gzip
import math
import runpy
import subprocess
from pathlib import Path

import moderngl
import numpy as np

RASTER = runpy.run_path(str(Path(__file__).with_name("render-native-film.py")))
PUBLIC = RASTER["SOURCE"]["PUBLIC"]
ROOT = PUBLIC / "exhibits/trouser-study"
WIDTH, HEIGHT, FPS = RASTER["WIDTH"], RASTER["HEIGHT"], RASTER["FPS"]
CARD = 1184
VIEW = (CARD, 0, WIDTH - CARD, HEIGHT)
STUDIES = {
    "posture": ("Bending the knee", ("standing", "squat-12", "squat-25"), "seatGap", "Mean seat gap"),
    "elastic": ("Holding the waist", ("elastic-5", "standing", "elastic-60"), "waistbandShift", "Waistband shift"),
    "section": ("Inside the folds", ("standing",), None, "Plane through the native mesh"),
}


def read_surface(layout, case):
    document = RASTER["SOURCE"]["document"]
    mesh = case.get("meshKey")
    topology = document(ROOT / f"{mesh}-topology.json" if mesh else PUBLIC / "exhibits/trousers.json")
    mesh_bytes = gzip.decompress((ROOT / (f"mesh-{mesh}.gz" if mesh else "mesh.gz")).read_bytes())
    raw = gzip.decompress((ROOT / case["surface"]).read_bytes())
    n_body, n_cloth = layout["bodyVertices"], case.get("clothVertices", layout["clothVertices"])
    indices = layout["bodyIndices"]
    position_bytes = 6 * (n_body + n_cloth)
    if len(raw) != position_bytes + 3 * n_cloth or len(mesh_bytes) != 2 * indices + 4 * n_cloth:
        raise ValueError(f"Mismatched surface and topology for {case['id']}")
    xyz = np.frombuffer(raw[:position_bytes], dtype="<i2").reshape(-1, 3).astype("f") / 10000
    body, cloth = xyz[:n_body], xyz[n_body:]
    body_faces = np.frombuffer(mesh_bytes, dtype="<u2", count=indices).reshape(-1, 3)
    cloth_faces = np.array(topology["triangles"], dtype=np.int32).reshape(-1, 3)
    grain = np.frombuffer(mesh_bytes, dtype="<i2", offset=2*indices).reshape(-1, 2).astype("f") / 10000
    if np.max(body_faces) >= len(body) or np.max(cloth_faces) >= len(cloth):
        raise ValueError(f"Invalid mesh indices for {case['id']}")
    return (body, body_faces), (cloth, cloth_faces), grain


def section(mesh, height):
    """Draw the same half-open horizontal intersections used by the live section view."""
    points, faces = mesh
    triangles = points[faces]
    following = np.roll(triangles, -1, axis=1)
    crossed = (triangles[:,:,2] > height) != (following[:,:,2] > height)
    a, b = triangles[crossed], following[crossed]
    if not len(a):
        return a
    amount = ((height - a[:,2]) / (b[:,2] - a[:,2]))[:,None]
    hits = a + (b-a) * amount
    hits[:,2] = 0
    return hits


def turned_view(bounds, yaw):
    facing = np.array([math.sin(yaw), -math.cos(yaw), .12])
    facing /= np.linalg.norm(facing)
    right = np.cross([0,0,1], facing)
    right /= np.linalg.norm(right)
    axes = np.stack((right, np.cross(facing, right), facing))
    coordinates = bounds @ axes.T
    low, high = coordinates.min(axis=0), coordinates.max(axis=0)
    aspect = VIEW[2] / HEIGHT
    height = max((high[1]-low[1])*.65, (high[0]-low[0])*.61/aspect)
    return axes, (low+high)/2 @ axes, (height*aspect, height)


def film_text(title, timed, section_film):
    def text(value, size, x, y, color, font="sans-serif", enable=None):
        filter_ = f"drawtext=font='{font}':text='{value}':fontsize={size}:fontcolor={color}:x={x}:y={y}"
        return filter_ + (f":enable='{enable}'" if enable else "")
    filters = [text("FRANKENTEXTILES / TROUSER STUDY", 36, 118, 165, "0x665e4a", "monospace"),
               text(title, 107, 112, 300, "0x2b4236", "Liberation Serif")]
    if section_film:
        filters += [text(r"%{eif\:round(150+156*(1-cos(t*60*PI/359)))\:d} mm above sole", 57, 118, 490, "0x365b51"),
                    text("Horizontal section through the solver mesh", 38, 118, 615, "0x365b51"),
                    text("BODY", 35, 118, 1725, "0x466a5f", "monospace"),
                    text("CLOTH", 35, 370, 1725, "0x9c4b2e", "monospace")]
    for i, (name, value) in enumerate(timed):
        during = f"gte(t,{2*i})*lt(t,{2*i+2})"
        filters += [text(name, 67, 118, 535, "0x365b51", enable=during),
                    text(value, 47, 118, 670, "0x365b51", enable=during)]
    return ",".join(filters)


def render(args):
    title, ids, metric, caption = STUDIES[args.study]
    section_film = args.study == "section"
    document = RASTER["SOURCE"]["document"]
    layout = document(ROOT / "study.json")
    cases = {case["id"]: case for case in layout["cases"] if case["id"] in ids}
    context = moderngl.create_standalone_context(backend="egl")
    raster = context.simple_framebuffer((WIDTH, HEIGHT), components=3)
    raster.use()
    shade = context.program(vertex_shader=RASTER["VERTEX"], fragment_shader=RASTER["FRAGMENT"])
    depth = context.program(vertex_shader=RASTER["DEPTH_VERTEX"], fragment_shader="#version 330\nvoid main() {}")
    shadow_texture = context.depth_texture((1024, 1024))
    shadow_texture.repeat_x = shadow_texture.repeat_y = False
    shadow_texture.compare_func = "<="
    shadow_texture.filter = moderngl.LINEAR, moderngl.LINEAR
    shadow_texture.use(location=1)
    shadow = context.framebuffer(depth_attachment=shadow_texture)
    backdrop = context.program(vertex_shader=RASTER["BACKGROUND_VERTEX"], fragment_shader=RASTER["BACKGROUND_FRAGMENT"])
    sky = context.vertex_array(backdrop, [])
    mark = context.program(vertex_shader=RASTER["MARK_VERTEX"], fragment_shader=RASTER["MARK_FRAGMENT"])
    lines = context.buffer(reserve=2**20, dynamic=True)
    wire = context.vertex_array(mark, [(lines, "3f", "position")])
    mark["round_point"].value = False
    swatch, pitch, tint = RASTER["fabric_texture"](context, "trousers")
    swatch.use(location=0)
    shade["cloth_image"].value = 0
    shade["shadow_image"].value = 1
    shade["fabric_pitch"].value = pitch
    shade["shadows"].value = True

    def surface(points, faces, uv, colour, caster=True):
        return RASTER["Surface"](context, shade, points, faces, uv, np.tile(colour, (len(points),1)), depth if caster else None)

    models = {}
    for id_ in ids:
        body_mesh, cloth_mesh, grain = read_surface(layout, cases[id_])
        body, faces = RASTER["trim_top"](*body_mesh, .119)
        faces = RASTER["main_component"](faces, len(body))
        form = surface(body, faces, body[:, [0,2]], (.49,.38,.26))
        cloth = surface(*cloth_mesh, grain, (.10,.17,.26))
        bounds = np.vstack((body[np.unique(faces)], cloth_mesh[0]))
        z = body_mesh[0][:,2].min()-.001
        floor = np.array([(-12,-12,z), (12,-12,z), (12,12,z), (-12,12,z)])
        stage = surface(floor, np.array([(0,1,2),(0,2,3)]), floor[:,:2], (.20,.17,.14), caster=False)
        models[id_] = form, cloth, stage, bounds, body_mesh, cloth_mesh

    def stroke(points, color, thickness):
        if len(points):
            lines.write(np.asarray(points, dtype="f").tobytes())
            mark["colour"].value = color
            context.line_width = thickness
            wire.render(mode=moderngl.LINES, vertices=len(points))

    timed = []
    if not section_film:
        for id_ in ids:
            case = cases[id_]
            name = f"{case['stiffness']} N elastic" if args.study == "elastic" else case["label"]
            value = case[metric]
            timed.append((name, f"{caption} {value:+.1f} mm" if args.study == "elastic" else f"{caption} {value:.1f} mm"))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    timeline = f"setpts=PTS+{args.still}/({FPS}*TB)," if args.still is not None else ""
    last_case = None
    with subprocess.Popen([
        "ffmpeg", "-nostdin", "-v", "error", "-n", "-filter_threads", "2", "-f", "rawvideo", "-pixel_format", "rgb24",
        "-video_size", f"{WIDTH}x{HEIGHT}", "-framerate", str(FPS), "-i", "pipe:0", "-an",
        "-vf", "vflip," + timeline + film_text(title, timed, section_film),
        *(["-frames:v", "1", "-threads", "1", "-update", "1"] if args.still is not None else [
            "-c:v", "libx264", "-threads", "4", "-preset", "veryfast", "-crf", "17", "-maxrate", "9M", "-bufsize", "9M",
            "-profile:v", "high", "-level:v", "5.2", "-pix_fmt", "yuv420p", "-color_range", "tv", "-movflags", "+faststart",
        ]), str(args.out),
    ], stdin=subprocess.PIPE) as output:
        for frame in ([args.still] if args.still is not None else range(6*FPS)):
            t = frame / (6*FPS-1)
            id_ = ids[0 if section_film else frame // (2*FPS)]
            form, cloth, stage, bounds, body, garment = models[id_]
            if section_film:
                yaw = .42 + .56 * math.sin(math.pi*t)
            else:
                # Repeat the same camera sweep on each independently solved comparison.
                shot = (frame % (2*FPS)) / (2*FPS-1)
                yaw = (.90 if args.study == "posture" else .22) + .76 * shot
            if id_ != last_case:
                light, centre, distance = RASTER["orthogonal_light"](bounds)
                for shader in (shade, depth):
                    shader["light_axes"].write(light.T.astype("f").tobytes())
                    shader["light_centre"].value = tuple(centre)
                    shader["light_extent"].value = distance
                shadow.use()
                shadow.clear(depth=1)
                context.enable(moderngl.DEPTH_TEST)
                form.depth.render()
                cloth.depth.render()
                last_case = id_
            raster.use()
            context.viewport = (0,0,WIDTH,HEIGHT)
            raster.clear(.945,.925,.86,1, depth=1)
            context.viewport = VIEW
            context.disable(moderngl.DEPTH_TEST)
            sky.render(vertices=3)
            context.enable(moderngl.DEPTH_TEST)
            camera = turned_view(bounds, yaw)
            RASTER["projection"](shade, *camera)
            shade["fabric_mix"].value = 0
            shade["shadow_only"].value = True
            context.enable(moderngl.BLEND)
            context.blend_func = moderngl.SRC_ALPHA, moderngl.ONE_MINUS_SRC_ALPHA
            stage.vao.render()
            context.disable(moderngl.BLEND)
            shade["shadow_only"].value = False
            form.vao.render()
            shade["fabric_mix"].value = tint
            cloth.vao.render()
            if section_film:
                height = body[0][:,2].min() + round(150+156*(1-math.cos(math.pi*t)))/1000
                plane = np.asarray([(-.27,-.30,height), (.27,-.30,height), (.27,.16,height), (-.27,.16,height)])
                RASTER["projection"](mark, *camera)
                stroke(np.column_stack((plane, np.roll(plane,-1,axis=0))).reshape(-1,3), (.60,.86,.74), 3)
                context.disable(moderngl.DEPTH_TEST)
                context.viewport = (122, 445, CARD-244, 950)
                RASTER["projection"](mark, np.eye(3), (0.,-.07,0.), (.217,.217))
                stroke(section(body, height), (.27,.41,.37), 5)
                stroke(section(garment, height), (.65,.30,.17), 7)
            output.stdin.write(raster.read(components=3, alignment=1))
        output.stdin.close()
        if output.wait() != 0:
            raise RuntimeError("FFmpeg could not write the trouser film")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("study", choices=STUDIES)
    cli.add_argument("out", type=Path)
    cli.add_argument("--still", type=int, metavar="FRAME")
    options = cli.parse_args()
    if options.still is not None and not 0 <= options.still < 6*FPS:
        cli.error("choose a still frame from 0 through 359")
    if options.out.exists():
        cli.error("choose a new output file")
    render(options)
