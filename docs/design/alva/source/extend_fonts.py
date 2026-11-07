# INPUT: Pinned OFL Noto source, existing prototype fonts and explicit required codepoints
# OUTPUT: Extended 400/600 TTF/WOFF assets and provenance with existing-glyph preservation checks
# POS: Offline design font maintenance; never rewrites canonical SmallPen data or removes earlier glyphs.
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path

from fontTools.pens.recordingPen import DecomposingRecordingPen, RecordingPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT=Path(__file__).resolve().parents[1]


def normalize_line_metrics(font):
    """Use the source's typographic em metrics, retaining its Windows ink bounds.

    OpenType recommends hhea and OS/2 typo metrics agree. This is one family
    contract, not a per-label optical offset; outlines and advances stay intact.
    """
    os2=font['OS/2'];header=font['hhea']
    header.ascent=os2.sTypoAscender
    header.descent=os2.sTypoDescender
    header.lineGap=os2.sTypoLineGap
    os2.fsSelection |= 1 << 7


def prepare_ui_fonts(fonts):
    """Derive the formal UI role while the historical prototype family stays intact."""
    manifest=json.loads((fonts/'NOTICE.json').read_text())
    manifest['family']='Alva UI Sans SC'
    manifest['derivation']+=' Formal UI role renamed separately; adopts the source OS/2 typographic metrics, without changing outlines/advances.'
    for style in ['Regular','SemiBold']:
        font=TTFont(fonts/('AlvaPrototypeSansSC-'+style+'.ttf'));normalize_line_metrics(font)
        names={1:'Alva UI Sans SC',2:style,3:'Alva UI Sans SC '+style+' 1',4:'Alva UI Sans SC '+style,
            6:'AlvaUISansSC-'+style,16:'Alva UI Sans SC',17:style}
        for name in font['name'].names:
            if name.nameID in names:name.string=names[name.nameID].encode(name.getEncoding(),errors='replace')
        font.recalcTimestamp=False
        for ext in ['ttf','woff']:
            font.flavor=None if ext=='ttf' else ext
            font.save(fonts/('AlvaUISansSC-'+style+'.'+ext))
    for entry in manifest['files']:
        entry['file']=entry['file'].replace('AlvaPrototypeSansSC','AlvaUISansSC');p=fonts/entry['file']
        entry['bytes']=p.stat().st_size;entry['sha256']=hashlib.sha256(p.read_bytes()).hexdigest()
    manifest['lineMetrics']={'hheaAscender':880,'hheaDescender':-120,'lineGap':0,'useTypoMetrics':True,
        'source':'Existing Noto Sans SC OS/2 sTypo* values; Windows clipping extents remain 1160/288.',
        'specification':'https://learn.microsoft.com/en-us/typography/opentype/spec/recom',
        'scope':'Formal ui.font.family only; historical prototype font assets stay unchanged. No label/icon offsets.'}
    (fonts/'UI-NOTICE.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')


def shape(font,cp):
    glyph=font.getBestCmap()[cp];pen=RecordingPen();font.getGlyphSet()[glyph].draw(pen)
    return pen.value,font['hmtx'].metrics[glyph]


def extend(source,characters,backups,ui=False):
    fonts=ROOT/'fonts';notice='UI-NOTICE.json' if ui else 'NOTICE.json'
    prefix='AlvaUISansSC-' if ui else 'AlvaPrototypeSansSC-'
    manifest=json.loads((fonts/notice).read_text())
    assert hashlib.sha256(source.read_bytes()).hexdigest()==manifest['sourceSHA256']
    wanted=set(json.loads(characters.read_text()));backups.mkdir(exist_ok=True,parents=True);reports=[]
    for style,weight in [('Regular',400),('SemiBold',600)]:
        ttf=fonts/(prefix+style+'.ttf');woff=ttf.with_suffix('.woff')
        font=TTFont(ttf);before=font.getBestCmap().copy();missing=sorted(wanted-set(before))
        if not missing:continue
        for p in (ttf,woff):(backups/(p.name+'.before-ds2')).write_bytes(p.read_bytes())
        old={cp:shape(font,cp) for cp in before}
        original=instantiateVariableFont(TTFont(source),{'wght':weight},inplace=True);glyphs=original.getGlyphSet()
        for cp in missing:
            source_name=original.getBestCmap()[cp];recording=DecomposingRecordingPen(glyphs);glyphs[source_name].draw(recording)
            pen=TTGlyphPen(None);recording.replay(pen);name=f'uni{cp:04X}.ds2'
            assert name not in font.getGlyphOrder()
            font.setGlyphOrder(font.getGlyphOrder()+[name]);font['glyf'][name]=pen.glyph()
            font['hmtx'].metrics[name]=deepcopy(original['hmtx'].metrics[source_name])
            if 'vmtx' in font:font['vmtx'].metrics[name]=deepcopy(original['vmtx'].metrics[source_name])
            for cmap in font['cmap'].tables:
                if cmap.isUnicode() and hasattr(cmap,'cmap'):cmap.cmap[cp]=name
        if ui:normalize_line_metrics(font)
        font.recalcTimestamp=False;font.flavor=None;font.save(ttf);font=TTFont(ttf)
        assert all(shape(font,cp)==expected for cp,expected in old.items())
        font.recalcTimestamp=False;font.flavor='woff';font.save(woff);other=TTFont(woff)
        assert all(font[t].compile(font)==other[t].compile(other) for t in ['glyf','cmap','hmtx','hhea','vmtx','vhea','GSUB','GPOS'])
        reports.append({'weight':weight,'previousCodepoints':len(before),'currentCodepoints':len(font.getBestCmap()),
            'addedCodepoints':[f'U+{cp:04X}' for cp in missing],'preservedExistingOutlinesAndAdvanceWidths':True,'ttfWoffTablesEquivalent':True})
    for entry in manifest['files']:
        p=fonts/entry['file'];entry['bytes']=p.stat().st_size;entry['sha256']=hashlib.sha256(p.read_bytes()).hexdigest()
    if reports:
        manifest['designSystemGlyphExtension']={'scope':'Both SmallPen packages receive the extended resources; all prior glyph outlines/advances remain unchanged.','weights':reports}
        (fonts/notice).write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(reports,ensure_ascii=False))


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--characters',type=Path,required=True);p.add_argument('--backups',type=Path,required=True)
    p.add_argument('--ui',action='store_true',help='Extend the formal UI role, preserving the historical prototype family')
    a=p.parse_args();extend(a.source,a.characters,a.backups,a.ui)
