# INPUT: Pinned Lucide SVG/font outlines, stable Alva symbol mappings and fontTools
# OUTPUT: One Lucide icon subset for SmallPen with preserved canvas metrics and source provenance
# POS: Design-only format adaptation; original SVGs remain available for Go/HTML consumers.
import hashlib
import json
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path
from fontTools.ttLib import TTFont

ROOT=Path(__file__).resolve().parents[1]


def build():
    source_dir=ROOT/'fonts'/'lucide'
    source=json.loads((source_dir/'glyphs.json').read_text())
    assert source['unitsPerEm']==source['ascent']==1000 and source['descent']==0
    glyphs={};metrics={};cmap={};entries=[]
    glyphs['.notdef']=TTGlyphPen(None).glyph();metrics['.notdef']=(1000,0)
    for item in source['glyphs']:
        svg=source_dir/(item['lucide']+'.svg')
        assert hashlib.sha256(svg.read_bytes()).hexdigest()==item['svgSha256'],svg
        glyph='icon_'+item['name'].lower();cp=int(item['codepoint'][2:],16)
        assert cp not in cmap and glyph not in glyphs
        pen=TTGlyphPen(None)
        # Use upstream outlined paths, never redraw or trace individual symbols.
        # Convert cubic curves to TrueType quadratics within 0.006 SVG px.
        parse_path(item['path'],Cu2QuPen(pen,max_err=0.25,reverse_direction=False))
        outline=pen.glyph();outline.recalcBounds(None)
        bounds=BoundsPen(None);parse_path(item['path'],bounds)
        converted=BoundsPen(None);outline.draw(converted,None)
        assert all(abs(a-b)<1 for a,b in zip(converted.bounds,bounds.bounds)),item['name']
        assert min(converted.bounds[:2])>=0 and max(converted.bounds[2:])<=1000,item['name']
        glyphs[glyph]=outline
        # SVG owns optical positioning. Retain its inset, not per-glyph ink centering.
        metrics[glyph]=(1000,outline.xMin);cmap[cp]=glyph
        entries.append({k:item[k] for k in ('name','codepoint','character','lucide','svgSha256')})
    fb=FontBuilder(1000,isTTF=True);fb.setupGlyphOrder(list(glyphs));fb.setupCharacterMap(cmap)
    fb.setupGlyf(glyphs);fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=1000,descent=0,lineGap=0)
    fb.setupNameTable({
        'familyName':'Alva Interface Icons','styleName':'Regular',
        'uniqueFontIdentifier':'AlvaInterfaceIcons-Lucide-'+source['version'],
        'fullName':'Alva Interface Icons Regular','psName':'AlvaInterfaceIcons-Regular',
        'version':'Version 2.0',
        'copyright':'Lucide Icons and Contributors; Feather-derived artwork by Cole Bemis.',
        'licenseDescription':'ISC; MIT for Feather-derived icons. See lucide/LICENSE.',
        'licenseInfoURL':'https://lucide.dev/license',
    })
    fb.setupOS2(sTypoAscender=1000,sTypoDescender=0,sTypoLineGap=0,usWinAscent=1000,usWinDescent=0)
    fb.setupPost();fb.setupMaxp();fb.font.recalcTimestamp=False
    fb.font['head'].created=fb.font['head'].modified=2082844800
    out=ROOT/'fonts';files=[]
    for suffix,flavor in [('ttf',None),('woff','woff')]:
        file=out/('AlvaInterfaceIcons-Regular.'+suffix)
        fb.font.flavor=flavor;fb.font.save(file)
        files.append({'file':file.name,'bytes':file.stat().st_size,'sha256':hashlib.sha256(file.read_bytes()).hexdigest()})
    ttf=TTFont(out/'AlvaInterfaceIcons-Regular.ttf');woff=TTFont(out/'AlvaInterfaceIcons-Regular.woff')
    for table in ('glyf','cmap','hmtx','hhea'):
        assert ttf[table].compile(ttf)==woff[table].compile(woff),table
    manifest={
        'family':'Alva Interface Icons','grid':24,'stroke':2,
        'provenance':'Lucide '+source['version']+' only; official outlined SVG glyphs, stable Alva names/codepoints, original 24-unit canvas and no per-icon positioning edits.',
        'license':'ISC AND MIT','licenseFile':'lucide/LICENSE',
        'source':{k:source[k] for k in ('package','version','tarball','integrity','fontSvgSha256')},
        'derivation':'Subset official font SVG paths; cubic-to-quadratic conversion <=0.25/1000 units; preserve 1000 advance/ascent, zero descent and xMin side bearings.',
        'glyphs':entries,'files':files,
    }
    (out/'icons.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    print('Built',len(entries),'Lucide symbols with stable mappings and preserved canvas metrics.')


if __name__=='__main__':build()
