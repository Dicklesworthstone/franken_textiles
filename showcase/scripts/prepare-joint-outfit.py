"""Sew the bolero and trousers in one native contact system. Run from the repository root."""
import argparse
import gzip
import hashlib
import json
import struct
import subprocess
from pathlib import Path
from runpy import run_path

import numpy as np

public=Path("showcase/public")
shared=run_path("showcase/scripts/prepare-drape-room.py")
cuts=["bolero","trousers"]

def inputs():
    swatch=json.loads(Path("tests/fixtures/materials/synthetic-joint-outfit.json").read_text())["garments"][0]["pattern"]["fabric"]
    garments=[]
    for name in cuts:
        pattern=json.loads((public/f"patterns/{name}.json").read_text())
        pattern["fabric"]=swatch
        if name=="bolero":
            for piece in pattern["pieces"]:
                place=piece["placement"]
                place["y0"]=place.get("y0",0)-50
        garments.append({"id":name,"pattern":pattern})
    return {"schema":"ftx-outfit-drape/1","garments":garments,"interfaces":[]}

def package(folder,output):
    report=json.loads((folder/"sim_report.json").read_text())
    request=json.loads((folder/"outfit_request.json").read_text())
    assert request==inputs() and not report["gating"] and report["numerical"]["settled"] and report["self_contact"]["enabled"]
    body,faces=shared["read_obj"](folder/"body.obj")
    packed=gzip.decompress((public/"exhibits/poses/520-35.bin.gz").read_bytes())
    count,indices=struct.unpack_from("<II",packed)
    used,remap=np.unique(faces,return_inverse=True)
    assert count==len(used) and indices==faces.size and packed[8:8+count*6]==shared["quantize"](body[used])
    assert packed[8+count*6:8+count*6+indices*2]==remap.astype("<u2").tobytes()
    cache=(folder/"fold.pc2").read_bytes()
    magic,version,n,_,_,frames=struct.unpack_from("<12siiffi",cache)
    assert (magic,version)==(b"POINTCACHE2\0",1)
    xyz=np.frombuffer(cache,dtype="<f4",offset=32).reshape(frames,n,3)
    endpoint,tri=shared["read_obj"](folder/"garment.obj")
    assert np.array_equal(endpoint[:,:3].astype("<f4"),xyz[-1])
    layouts=[json.loads((public/f"exhibits/{name}.json").read_text()) for name in cuts]
    assert sum(x["vertices"] for x in layouts)==n
    assert np.array_equal(tri.flatten(),np.concatenate([layouts[0]["triangles"],np.array(layouts[1]["triangles"])+layouts[0]["vertices"]]))
    phases=json.loads((folder/"mesh_map.json").read_text())["frames"]
    assert frames==sum(phases.values()) and phases["sew"]+phases["drape"]==sum(report["frames"].values())
    # Coordinates as float32 exactly as written by the native PC2. Both garments use the same frame.
    payload=b"".join([struct.pack("<II",len(used),faces.size),body[used].astype("<f4").tobytes(),
                     remap.astype("<u4").tobytes(),xyz.tobytes()])
    output.mkdir(parents=True,exist_ok=True)
    (output/"ensemble.gz").write_bytes(gzip.compress(payload,mtime=0))
    meta={"case":"520-35","frames":frames,"phases":phases,"counts":[x["vertices"] for x in layouts],
          "waistbandShift":report["bands"]["trousers::waistband"]["shift_mm"],"strain":report["global_strain_pct"],
          "settled":report["numerical"]["settled"],"gating":report["gating"],"advisories":report["flags"],
          "selfContact":report["self_contact"]["enabled"],"pc2Sha256":hashlib.sha256(cache).hexdigest(),
          "contactBasis":report["outfit"]["contact_basis"],"patternSha256":report["outfit"]["composed_pattern_sha256"]}
    (output/"ensemble.json").write_text(json.dumps(meta,indent=2)+"\n")
    (output/"ensemble-request.json").write_bytes((folder/"outfit_request.json").read_bytes())

if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory",type=Path);parser.add_argument("output",type=Path);parser.add_argument("--solve",action="store_true")
    args=parser.parse_args()
    if args.solve:
        args.directory.mkdir(parents=True,exist_ok=False)
        request=args.directory/"request.json";request.write_text(json.dumps(inputs()))
        tapes=args.directory/"tapes.json";tapes.write_text(json.dumps(shared["synthetic_tapes"](520)))
        dest=args.directory/"drape"
        subprocess.run(["target/release/ftx","drape",request,"--measurements",tapes,"--out",dest,
                        "--self-contact-mm","2","--spacing-mm","12","--arm-deg","35","--json"],check=True,stdout=subprocess.DEVNULL)
    else:dest=args.directory
    package(dest,args.output)
