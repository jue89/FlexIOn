#!/usr/bin/env python

import sys
import os
import horizon
import pyexcel

def ensure_dir(path):
    os.path.isdir(path) or os.makedirs(path)

if not sys.argv[2:]:
    print('No outdir and version given')
    sys.exit(1)

indir = os.path.dirname(os.path.realpath(__file__))
outdir = sys.argv[1];
version = sys.argv[2];

# Prepare environment
os.chdir(indir)
ensure_dir(outdir)

prj = horizon.Project('./flexion.hprj')
sch = prj.open_top_schematic()
brd = prj.open_board()

# Export schematic
sch.export_pdf({
    'min_line_width': 0,
    'output_filename': '%s/schematic-%s.pdf' % (outdir, version)
})

# Export 3D rendering
exporter = brd.export_3d(1920, 1080) #width, height
exporter.view_all()
exporter.load_3d_models()
exporter.render_to_png('%s/board-%s.png' % (outdir, version))

# Export Gerber
brd.export_gerber({
    'drill_mode': 'merged',
    'drill_npth': '',
    'drill_pth': '.XLN',
    'layers': {
        '-1': {'enabled': True, 'filename': '.G2L', 'layer': -1},
        '-100': {'enabled': True, 'filename': '.GBL', 'layer': -100},
        '-110': {'enabled': True, 'filename': '.GBS', 'layer': -110},
        '-120': {'enabled': True, 'filename': '.GBO', 'layer': -120},
        '-130': {'enabled': True, 'filename': '.GBP', 'layer': -130},
        '-2': {'enabled': True, 'filename': '.G3L', 'layer': -2},
        '0': {'enabled': True, 'filename': '.GTL', 'layer': 0},
        '10': {'enabled': True, 'filename': '.GTS', 'layer': 10},
        '100': {'enabled': True, 'filename': '.GKO', 'layer': 100},
        '20': {'enabled': True, 'filename': '.GTO', 'layer': 20},
        '30': {'enabled': True, 'filename': '.GTP', 'layer': 30}
    },
    'output_directory': outdir,
    'prefix': 'flexion-%s' % version,
    'zip_output': True
})

# Generate BOM
bom_path = '%s/flexion-%s-bom.csv' % (outdir, version)
sch.export_bom({
    'concrete_parts': {},
    'csv_settings': {
        'column_names': {
            'MPN': 'MPN',
            'QTY': '',
            'datasheet': '',
            'description': 'Description',
            'manufacturer': 'Manufacturer',
            'package': 'Package',
            'refdes': 'Designator',
            'value': 'Value'
        },
        'columns': ['refdes', 'value', 'description', 'package', 'MPN'],
        'custom_column_names': True,
        'order': 'asc',
        'sort_column': 'refdes'
    },
    'include_nopopulate': False,
    'orderable_MPNs': {},
    'output_filename': bom_path
})

# Generate PNP data
brd.export_pnp({
    'column_names': {
        'MPN': '',
        'angle': 'Rotation',
        'manufacturer': '',
        'package': '',
        'refdes': 'Designator',
        'side': 'Layer',
        'value': '',
        'x': 'Mid X',
        'y': 'Mid Y'
    },
    'columns': ['refdes', 'x', 'y', 'side', 'angle'],
    'customize': True,
    'mode': 'merged',
    'filename_top': '',
    'filename_bottom': '',
    'filename_merged': 'flexion-%s-pnp.csv' % version,
    'output_directory': outdir,
    'position_format': '%.4m',
    'bottom_side': 'Bottom',
    'top_side': 'Top'
})

# Extract MPNs
refdes2mpn = {}
for row in pyexcel.get_records(file_name = bom_path):
    for refdes in row['Designator'].split(', '):
        refdes2mpn[refdes] = row['MPN']

# Load JLC fab data
jlc_data = {}
for row in pyexcel.get_records(file_name = './jlc-data.csv'):
    mpn = row['MPN']
    jlc_data[mpn] = {
        'order_no': row['OrderNo'],
        'x': float(row['OffsetX']),
        'y': float(row['OffsetY']),
        'rot': float(row['OffsetRot']),
    }

# Generate JLC BOM
jlc_bom = []
for row in pyexcel.get_records(file_name = bom_path):
    mpn = row['MPN']
    if not mpn in jlc_data:
        print("BOM: Skip %s" % mpn)
        continue
    jlc_bom.append({
        'Comment': '%s %s' % (row['Value'], row['Description']),
        'Designator': row['Designator'],
        'Footprint': row['Package'],
        'JLCPCB Part #': jlc_data[mpn]['order_no']
    })
pyexcel.save_as(records = jlc_bom, dest_file_name = '%s/flexion-%s-bom-jlc.csv' % (outdir, version), dest_delimiter = ',')

# Generate JLC PNP
jlc_pnp = []
for row in pyexcel.get_records(file_name = '%s/flexion-%s-pnp.csv' % (outdir, version)):
    refdes = row['Designator']
    if not refdes in refdes2mpn:
        print("PNP: Skip %s" % refdes)
        continue
    mpn = refdes2mpn[refdes]
    if not mpn in jlc_data:
        print("PNP: Skip %s" % refdes)
        continue
    offset = jlc_data[mpn]
    # TODO: Compensate rotation before adding x/y offset
    x = float(row['Mid X']) + offset['x']
    y = float(row['Mid Y']) + offset['y']
    rot = float(row['Rotation']) + offset['rot']
    jlc_pnp.append({
        'Designator': refdes,
        'Mid X': '%.4fmm' % x,
        'Mid Y': '%.4fmm' % y,
        'Layer': row['Layer'],
        'Rotation': '%.0f' % rot
    })
pyexcel.save_as(records = jlc_pnp, dest_file_name = '%s/flexion-%s-pnp-jlc.csv' % (outdir, version), dest_delimiter = ',')
