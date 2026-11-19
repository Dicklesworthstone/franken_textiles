"""Build each interpretive figure's visual hull from its eight illustrated elevations.

The bundled RGBA atlas is the source for both the WebGL colour and the silhouette
intersection. Rebuild with Python, Pillow, numpy, scipy, OpenCV, scikit-image and fast-simplification:
  python prepare-hussar-studies.py blue --output NEW_DIR --atlas public/wardrobe/hussar/blue-views.webp
Original four-view illustration sheets can instead be registered with --cardinal and --diagonal.
"""
import argparse
import gzip
import struct
from pathlib import Path

import cv2
import fast_simplification
import numpy as np
from PIL import Image
from scipy import ndimage as nd
from skimage.measure import marching_cubes


WIDTH, HEIGHT = 420, 960
FLOOR, SCALE = 946, 4.5  # atlas pixels per exhibit centimetre


def tiles(source):
    image = np.asarray(Image.open(source).convert("RGBA"))
    opaque = image[:, :, 3] > 128
    # The four people have unequal margins. Split only in the empty gutters.
    occupied = np.flatnonzero(np.any(opaque, axis=0))
    gaps = np.argsort(np.diff(occupied))[-3:]
    intervals = np.split(occupied, np.sort(gaps) + 1)
    for columns in intervals:
        left, right = int(columns[0]), int(columns[-1])
        cut = image[:, left:right + 1]
        alpha = cut[:, :, 3] > 128
        top, bottom = np.where(alpha)[0][[0, -1]]
        midpoints = []
        for y in range(int(top + .77 * (bottom - top)), int(top + .89 * (bottom - top))):
            xs = np.flatnonzero(alpha[y])
            if xs.size:
                midpoints.append((xs[0] + xs[-1]) / 2)
        cx = np.median(midpoints)
        scale = 930 / (bottom - top)
        picture = Image.fromarray(cut).resize((round(cut.shape[1]*scale), round(cut.shape[0]*scale)), Image.Resampling.LANCZOS)
        tile = Image.new("RGBA", (WIDTH, HEIGHT))
        tile.paste(picture, (round(WIDTH/2 - cx*scale), round(FLOOR - bottom*scale)))
        array = np.array(tile)
        matte = array[:, :, 3] > 80
        # Leave a colour bleed for bilinear sampling of a surface at its silhouette.
        near = nd.distance_transform_edt(~matte, return_distances=False, return_indices=True)
        array[:, :, :3] = array[near[0], near[1], :3]
        array[:, :, 3] = np.maximum(array[:, :, 3], 3)
        yield Image.fromarray(array)


def register_plumes(views):
    arrays = np.stack([np.array(v) for v in views])
    # The narrow plume does not provide a stable anchor for centring the whole figure. Register
    # just that part to a common 3D centreline before intersecting the silhouettes.
    rot = np.arange(8)*np.pi/4
    basis = np.stack((np.cos(rot), -np.sin(rot)), axis=1)
    length = 157
    centres = np.empty((8,length))
    for i, view in enumerate(arrays):
        strip = view[:length,:,3] > 70
        for y in range(length):
            xs = np.flatnonzero(strip[y])
            centres[i,y] = np.mean(xs)-WIDTH/2 if xs.size else np.nan
        valid=np.isfinite(centres[i])
        centres[i]=np.interp(np.arange(length),np.where(valid)[0],centres[i,valid])
    expected = basis @ np.linalg.lstsq(basis,centres,rcond=None)[0]
    fades = np.clip((147-np.arange(length))/35,0,1)
    shifts = nd.gaussian_filter1d(expected-centres,2,axis=1)*fades
    for i, view in enumerate(arrays):
        for y, shift in enumerate(np.rint(shifts[i]).astype(int)):
            view[y] = np.roll(view[y],shift,axis=0)
    return [Image.fromarray(v) for v in arrays]


def write_correspondences(image, xyz, destination, name):
    masks, grays, depths = [], [], []
    density = 1.5
    x,y,z = xyz.T
    screen_y, screen_x = np.mgrid[:HEIGHT,:WIDTH].astype("f")
    u=(screen_x-WIDTH/2)/SCALE
    cv2.setNumThreads(2)
    for i in range(8):
        a=i*np.pi/4
        depth=np.full((int(HEIGHT/density)+1,int(WIDTH/density)+1),-70,dtype="f")
        xp=np.rint((WIDTH/2+SCALE*(x*np.cos(a)-z*np.sin(a)))/density).astype(int)
        yp=np.rint((FLOOR-SCALE*y)/density).astype(int)
        use=(xp>=0)&(yp>=0)&(xp<depth.shape[1])&(yp<depth.shape[0])
        np.maximum.at(depth,(yp[use],xp[use]),(x*np.sin(a)+z*np.cos(a))[use])
        depth=nd.maximum_filter(depth,size=3)
        depths.append(cv2.resize(depth,(WIDTH,HEIGHT),interpolation=cv2.INTER_LINEAR))
        row,col=divmod(i,4)
        view=image[row*HEIGHT:(row+1)*HEIGHT,col*WIDTH:(col+1)*WIDTH]
        matte=view[:,:,3].astype("f")/255
        masks.append(matte)
        gray=cv2.cvtColor(view[:,:,:3],cv2.COLOR_RGB2GRAY)
        grays.append(np.rint(gray*matte+100*(1-matte)).astype("B"))
    flow=cv2.DISOpticalFlow_create(cv2.DISOPTICAL_FLOW_PRESET_MEDIUM)
    flow.setFinestScale(0)
    output=np.full(image.shape,128,dtype="B")
    for i in range(8):
        a=i*np.pi/4
        px=u*np.cos(a)+depths[i]*np.sin(a)
        pz=-u*np.sin(a)+depths[i]*np.cos(a)
        maps=[]
        for neighbour in ((i-1)%8,(i+1)%8):
            # Render the adjacent illustration onto this elevation using the actual hull depth,
            # then register its remaining image displacement (belt edge, eye, buttons, fingers).
            b=neighbour*np.pi/4
            sx=(WIDTH/2+SCALE*(px*np.cos(b)-pz*np.sin(b))).astype("f")
            projected=cv2.remap(grays[neighbour],sx,screen_y,cv2.INTER_LINEAR,borderMode=cv2.BORDER_REPLICATE)
            valid=depths[i]>-60
            projected[~valid]=100
            delta=flow.calc(projected,grays[i],None)
            delta*=np.clip(masks[i]*2,0,1)[:,:,None]
            maps.append(np.rint(np.clip(delta,-42,42)*3+128).astype("B"))
        row,col=divmod(i,4)
        output[row*HEIGHT:(row+1)*HEIGHT,col*WIDTH:(col+1)*WIDTH]=np.concatenate(maps,axis=2)
    with (destination / (name+"-flow.webp")).open("xb") as file:
        Image.fromarray(output).save(file,format="WEBP",lossless=True,method=5,exact=True)

def build(name, atlas, destination):
    image = np.array(atlas)
    if atlas.size != (4*WIDTH, 2*HEIGHT):
        raise ValueError("Unexpected elevation atlas dimensions")
    step = .70
    x, y, z = np.meshgrid(np.arange(-49, 49+step, step), np.arange(-2, 211+step, step),
                          np.arange(-46, 46+step, step), indexing="ij", sparse=True)
    distances = []
    for i in range(8):
        row, col = divmod(i, 4)
        mask = image[row*HEIGHT:(row+1)*HEIGHT, col*WIDTH:(col+1)*WIDTH, 3] > 100
        field = (nd.distance_transform_edt(mask) - nd.distance_transform_edt(~mask)).astype("f")
        angle = i*np.pi/4
        u = np.rint(WIDTH/2 + SCALE*(x*np.cos(angle) - z*np.sin(angle))).astype(int)
        v = np.rint(FLOOR-SCALE*y).astype(int)
        distances.append(field[np.clip(v, 0, HEIGHT-1), np.clip(u, 0, WIDTH-1)])
    ranked = np.partition(np.stack(distances), (1, 2), axis=0)
    # Small independent source differences at a finger, feather or fur trim should not cut a hole
    # through the figure. The two most restrictive corroborating silhouettes define the surface.
    hull = nd.gaussian_filter(.70*ranked[1] + .30*ranked[2], .95)
    inside=np.argwhere(hull>0)*step + [-49,-2,-46]
    write_correspondences(image,inside,destination,name)
    vertices, faces, _, _ = marching_cubes(hull, level=0, spacing=(step,step,step))
    vertices += [-49, -2, -46]
    vertices, faces = fast_simplification.simplify(vertices.astype("d"), faces.astype("i"), target_count=40000)
    # marching_cubes returns clockwise outward faces in the native x/y/z frame.
    faces = faces[:, ::-1]
    data = struct.pack("<II", len(vertices), len(faces)) + np.rint(vertices*100).astype("<i2").tobytes() + faces.astype("<u2").tobytes()
    with (destination / (name + "-hull.gz")).open("xb") as file:
        file.write(gzip.compress(data, mtime=0))
    print(f"{name}: {len(vertices)} vertices, {len(faces)} triangles")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("regiment", choices=["blue","yellow"])
    p.add_argument("--atlas", type=Path)
    p.add_argument("--cardinal", type=Path)
    p.add_argument("--diagonal", type=Path)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    args.output.mkdir(exist_ok=True, parents=True)
    if args.atlas:
        registered = Image.open(args.atlas).convert("RGBA")
    else:
        if not args.cardinal or not args.diagonal:
            p.error("both source sheets are required to make an atlas")
        registered = Image.new("RGBA", (WIDTH*4,HEIGHT*2))
        views=[view for pair in zip(tiles(args.cardinal),tiles(args.diagonal),strict=True) for view in pair]
        for i,tile in enumerate(register_plumes(views)):
            row,col=divmod(i,4)
            registered.paste(tile,(col*WIDTH,row*HEIGHT))
        path = args.output / (args.regiment + "-views.webp")
        with path.open("xb") as out:
            registered.save(out,format="WEBP",quality=94,method=6,exact=True)
        registered = Image.open(path).convert("RGBA")
    build(args.regiment, registered, args.output)
