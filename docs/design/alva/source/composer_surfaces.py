# INPUT: Existing Composer screens and the former per-page overlay examples
# OUTPUT: Shared-component pages with duplicate overlay roots and their prototype links removed
# POS: Pure authoring transformation; publish results only through revision-guarded public CLI operations.
from copy import deepcopy
import uuid


LEGACY_SURFACES = frozenset([
    'PopoverPermission', 'PopoverAdd', 'PopoverReferences', 'PopoverCommands',
    'PermissionHintAsk', 'PermissionHintAuto', 'PermissionHintFull',
    'HoverPermission', 'HoverModel', 'HoverVoice', 'HoverSend',
])


def consolidate(screen):
    """Retire only the known generated examples; retain the actual page tree.

    Tooltip/menu visuals belong to Foundation, with one Add composition on 41.
    Invocation/content belong to the Product's native requirements/annotations.
    Do not generate a FRAME and cloned trigger for each tooltip string.
    """
    screen = deepcopy(screen)
    reports = []
    for presentation in screen['presentations']:
        nodes = presentation['nodes']
        namespace = uuid.uuid5(uuid.NAMESPACE_URL, screen['id']+'/'+presentation['id']+'/composer')
        roots = presentation.get('rootIds', [presentation['rootId']])
        targets = [rid for rid in roots if nodes[rid]['name'] in LEGACY_SURFACES
            and rid == 'node_'+uuid.uuid5(namespace, nodes[rid]['name']).hex]
        removed = set()

        def collect(node_id):
            if node_id in removed:
                return
            removed.add(node_id)
            for child in nodes[node_id].get('children', []):
                collect(child)

        for node_id in targets:
            collect(node_id)
        if not removed:
            continue
        # A component sample must not share canonical children with the page.
        for node_id, node in nodes.items():
            if node_id not in removed:
                assert not removed.intersection(node.get('children', [])), node_id
        references = removed | {str(uuid.UUID(hex=node_id[5:])) for node_id in removed}

        def points_to_removed(value):
            if isinstance(value, str):
                return value in references
            if isinstance(value, list):
                return any(points_to_removed(item) for item in value)
            if isinstance(value, dict):
                return any(points_to_removed(item) for item in value.values())
            return False

        pruned = 0

        def keep_interactions(interactions):
            nonlocal pruned
            kept = [item for item in interactions if not points_to_removed(item)]
            pruned += len(interactions)-len(kept)
            return kept

        for node_id, node in nodes.items():
            if node_id in removed:
                continue
            if 'interactions' in node:
                node['interactions'] = keep_interactions(node['interactions'])
            overrides = node.get('instance', {}).get('overrides', {})
            for path, value in list(overrides.items()):
                if path.endswith(':interactions'):
                    kept = keep_interactions(value)
                    if kept:
                        overrides[path] = kept
                    else:
                        del overrides[path]
        if 'interactions' in presentation:
            presentation['interactions'] = keep_interactions(presentation['interactions'])
        reports.append({'presentationId': presentation['id'], 'removedRootNames': [nodes[rid]['name'] for rid in targets],
            'removedNodeIds': sorted(removed), 'removedTriggerInteractions': pruned})
        for node_id in removed:
            del nodes[node_id]
        remaining = [rid for rid in roots if rid not in removed]
        if len(remaining) == 1:
            presentation.pop('rootIds', None)
        else:
            presentation['rootIds'] = remaining
    return screen, reports
