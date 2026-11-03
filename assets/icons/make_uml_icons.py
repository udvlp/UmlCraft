"""Writes UmlCraft's UML tool icons in the style of Fluent UI System Icons, 24 px regular.

Shapes are laid out on a 20 px grid (content 2..18) and mapped onto Fluent's 24 px grid
(content 2..22) with 1.5 px strokes, as Fluent's own 24 px icons have. File names are the
palette's tool keys. Run `python make_uml_icons.py` to rewrite `uml/`.
"""
import io
import os
import re

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'uml')


def pos(v):
    return f'{1.25 * v - 0.5:g}'


def size(v):
    return f'{1.25 * v:g}'


def coords(d):
    return re.sub(r'-?\d+(?:\.\d+)?', lambda m: pos(float(m.group())), d)


HEAD = (r'<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#212121" '
        r'stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" '
        r'xmlns="http://www.w3.org/2000/svg">')
FILL = r' fill="#212121"'


def dash(on=2, off=1.5):
    return f' stroke-dasharray="{size(on)} {size(off)}" stroke-linecap="butt"'


def path(d, extra=''):
    return f'<path d="{coords(d)}"{extra}/>'


def dashed(d):
    return path(d, dash())


def rect(x, y, w, h, rx=0, extra=''):
    r = f' rx="{size(rx)}"' if rx else ''
    return f'<rect x="{pos(x)}" y="{pos(y)}" width="{size(w)}" height="{size(h)}"{r}{extra}/>'


def circle(cx, cy, r, extra=''):
    return f'<circle cx="{pos(cx)}" cy="{pos(cy)}" r="{size(r)}"{extra}/>'


def ellipse(cx, cy, rx, ry):
    return f'<ellipse cx="{pos(cx)}" cy="{pos(cy)}" rx="{size(rx)}" ry="{size(ry)}"/>'


def dot(cx, cy, r):
    return circle(cx, cy, r, FILL + ' stroke="none"')


BAR = rect(3, 9, 13, 2, extra=FILL + ' stroke="none"')
INITIAL = [circle(10, 10, 4.5, FILL)]
FINAL = [circle(10, 10, 6.5), dot(10, 10, 4)]
FORK = [BAR, path('M9.5 3.5V9M5.5 11V16.5M13.5 11V16.5')]
JOIN = [BAR, path('M5.5 3.5V9M13.5 3.5V9M9.5 11V16.5')]
PACKAGE = [path('M2.5 16.5V3.5H8.5V6.5H17.5V16.5Z'), path('M2.5 6.5H8.5')]
COMPONENT = [path('M4.5 5.5V3.5H17.5V16.5H4.5V14.5M4.5 8.5V11.5'),
             rect(2.5, 5.5, 4, 3), rect(2.5, 11.5, 4, 3)]
NODE = [path('M2.5 5.5H14.5V17.5H2.5Z'), path('M2.5 5.5L5.5 2.5H17.5L14.5 5.5'),
        path('M17.5 2.5V14.5L14.5 17.5')]
FRAGMENT = [rect(2.5, 3.5, 15, 13, 2), path('M2.5 7.5H7.5L9.5 5.5V3.5')]
OPEN_HEAD = path('M12.5 6L17.5 9.5L12.5 13')
MESSAGE_HEAD = path('M12.5 6.5L17.5 9.5L12.5 12.5Z', FILL)
NAVIGABLE = [path('M2.5 9.5H17.5'), OPEN_HEAD]
DIAMOND = 'M9.5 9.5L13.5 6.5L17.5 9.5L13.5 12.5Z'
NSD_ALTERNATIVE = [rect(2.5, 3.5, 15, 13), path('M2.5 3.5L10 9.5L17.5 3.5M2.5 9.5H17.5'),
                   path('M10 9.5V16.5')]
NSD_LOOP = [path('M2.5 3.5H17.5V7.5H6.5V16.5H2.5Z'), rect(6.5, 7.5, 11, 9)]
BOX = rect(2.5, 3.5, 15, 13, 2)


def band(top):
    """The lower part of a box with rounded bottom corners, from `top` down, filled: the
    compartment a member lives in."""
    return path(f'M2.5 {top}H17.5V14.5C17.5 15.6 16.6 16.5 15.5 16.5H4.5'
                f'C3.4 16.5 2.5 15.6 2.5 14.5Z', FILL)

ICONS = {
    # Nodes
    'class': [rect(2.5, 3.5, 15, 13, 2), path('M2.5 7.5H17.5M2.5 11.5H17.5')],
    'interface': [circle(13.5, 10.5, 4), path('M2.5 10.5H9.5')],
    'enumeration': [rect(2.5, 3.5, 15, 13, 2), path('M2.5 7.5H17.5'),
                    path('M5.5 10.5H14.5M5.5 13.5H11.5')],
    'package': PACKAGE,
    'subsystem': PACKAGE + [path('M10 9V11.5M7.5 14V11.5H12.5V14')],
    'comment': [path('M2.5 4.5H13.5L17.5 8.5V15.5H2.5Z'), path('M13.5 4.5V8.5H17.5')],
    'actor': [circle(9.5, 4.5, 2),
              path('M9.5 6.5V12.5M5.5 8.5H13.5M9.5 12.5L6 17.5M9.5 12.5L13 17.5')],
    'useCase': [ellipse(10, 10, 7.5, 4.5)],
    # Robustness analysis: a circle with a T on its side, with an arrow head on top, on a line.
    'boundary': [path('M2.5 6V14M2.5 10H6.5'), circle(12, 10, 5.5)],
    'control': [circle(10, 11, 6), path('M12.5 2.5L10 5L12.5 7.5')],
    'entity': [circle(10, 9, 6), path('M4 15.5H16')],
    'lifeline': [rect(3.5, 2.5, 12, 5, 1), dashed('M9.5 8V17.5')],
    'systemBoundary': [rect(2.5, 2.5, 15, 15, 2), path('M4.5 5.5H9.5'),
                       ellipse(10, 11.5, 4.5, 3)],
    # A condition on a lifeline: a rounded box across its dashed line.
    'stateInvariant': [dashed('M10 7V2.5'), rect(3.5, 7, 13, 6, 3), dashed('M10 13V17.5')],
    # A lifeline ending in a cross.
    'termination': [dashed('M10 9V2.5'), path('M6.5 10.5L13.5 17.5M13.5 10.5L6.5 17.5')],
    # Two messages and the time between them: an arrow at both ends.
    'durationConstraint': [path('M2.5 3.5H17.5M2.5 16.5H17.5'), path('M10 6V14'),
                           path('M7.5 8.5L10 6L12.5 8.5M7.5 11.5L10 14L12.5 11.5')],
    # A message with the line written on beyond its head, and a clock.
    'timeConstraint': [path('M2.5 5.5H8.5'), path('M8 3L11.5 5.5L8 8Z', FILL),
                       path('M11.5 5.5H17.5'), circle(10, 13, 4.5), path('M10 10.5V13H12.5')],
    'combinedFragment': FRAGMENT + [dashed('M3 11.5H17')],
    'interactionUse': FRAGMENT + [path('M6.5 11.5H13.5')],
    'action': [rect(2.5, 5.5, 15, 9, 3.5)],
    'initialNode': INITIAL,
    'activityFinal': FINAL,
    'flowFinal': [circle(10, 10, 6.5), path('M5.4 5.4L14.6 14.6M14.6 5.4L5.4 14.6')],
    'decision': [path('M9.5 7.5L13.5 11.5L9.5 15.5L5.5 11.5Z'),
                 path('M9.5 2.5V7.5M5.5 11.5H2.5M13.5 11.5H16.5')],
    'merge': [path('M9.5 4.5L13.5 8.5L9.5 12.5L5.5 8.5Z'),
              path('M2.5 8.5H5.5M13.5 8.5H16.5M9.5 12.5V17.5')],
    'fork': FORK,
    'join': JOIN,
    'objectNode': [rect(2.5, 5.5, 15, 9, 1)],
    # Actions of their own kind: UML's pentagon, notch and hourglass, and the call behavior
    # action's rake in an action.
    'sendSignal': [path('M2.5 5.5H13L17.5 10L13 14.5H2.5Z')],
    'acceptEvent': [path('M2.5 5.5H17.5V14.5H2.5L7 10Z')],
    'acceptTimeEvent': [path('M5.5 2.5H14.5L5.5 17.5H14.5Z')],
    'callBehavior': [rect(2.5, 4.5, 15, 11, 3.5), path('M11.5 13V9.5M8.5 13V11.5H14.5V13')],
    # A flow ending in a connector circle.
    'activityConnector': [circle(12.5, 10, 5), path('M2.5 10H7.5'), path('M5 7.5L7.5 10L5 12.5')],
    # Pins: a square on an action's border, a flow going in or coming out.
    'inputPin': [rect(10.5, 4.5, 7, 11, 2.5), rect(6.5, 8, 4, 4), path('M2.5 10H6.5')],
    'outputPin': [rect(2.5, 4.5, 7, 11, 2.5), rect(9.5, 8, 4, 4), path('M13.5 10H17.5')],
    # A box on the border of the activity's frame.
    'activityParameter': [path('M17.5 2.5H7.5V7.5M7.5 12.5V17.5H17.5'), rect(2.5, 7.5, 10, 5, 1)],
    'partition': [rect(2.5, 2.5, 15, 15, 2), path('M2.5 6.5H17.5M9.5 2.5V17.5')],
    'state': [rect(2.5, 4.5, 15, 11, 3), path('M2.5 8.5H17.5')],
    # A state with UML's submachine glyph: two rounded boxes joined by a line.
    'submachineState': [rect(2.5, 4.5, 15, 11, 3), path('M2.5 8.5H17.5'),
                        rect(4.5, 10.5, 4.5, 3, 1.5), rect(11, 10.5, 4.5, 3, 1.5),
                        path('M9 12H11')],
    # Points on the border of a state: a circle where a transition enters, a crossed one
    # where it leaves.
    'entryPoint': [path('M17.5 4.5H11.5C10.4 4.5 9.5 5.4 9.5 6.5'),
                   path('M9.5 13.5C9.5 14.6 10.4 15.5 11.5 15.5H17.5'),
                   circle(9.5, 10, 4), path('M2.5 10H5.5')],
    'exitPoint': [path('M2.5 4.5H8.5C9.6 4.5 10.5 5.4 10.5 6.5'),
                  path('M10.5 13.5C10.5 14.6 9.6 15.5 8.5 15.5H2.5'),
                  circle(10.5, 10, 4), path('M8.2 7.7L12.8 12.3M12.8 7.7L8.2 12.3'),
                  path('M14.5 10H17.5')],
    # A submachine state with a stub state's bar in it.
    'stubState': [rect(2.5, 4.5, 15, 11, 3), path('M2.5 8.5H17.5'),
                  rect(5.5, 11, 9, 2, extra=FILL + ' stroke="none"')],
    'finalState': FINAL,
    'initialPseudostate': INITIAL,
    'choice': [path('M10 3.5L16.5 10L10 16.5L3.5 10Z')],
    'junction': [circle(10.5, 10.5, 2.5, FILL),
                 path('M3 4.5L8.2 8.7M3 16.5L8.2 12.3M13.5 10.5H17.5')],
    'forkPseudostate': FORK,
    'joinPseudostate': JOIN,
    'shallowHistory': [circle(10, 10, 7.5), path('M7.5 6V14M12.5 6V14M7.5 9.5H12.5')],
    'deepHistory': [circle(10, 10, 7.5), path('M6.5 6.5V13.5M10.5 6.5V13.5M6.5 9.5H10.5'),
                    path('M13.5 6V10M11.8 7L15.2 9M11.8 9L15.2 7')],
    'component': COMPONENT,
    'artifact': [path('M4.5 2.5H11.5L15.5 6.5V17.5H4.5Z'), path('M11.5 2.5V6.5H15.5'),
                 path('M7 10.5H13M7 13.5H13')],
    'node': NODE,
    'componentInstance': COMPONENT + [path('M8 10.5H14')],
    'nodeInstance': NODE + [path('M5.5 11.5H11.5')],
    'object': [rect(2.5, 3.5, 15, 13, 2), path('M6.5 7.5H13.5M2.5 10.5H17.5')],
    # Composite structures: a part in its class, a port on a border, a line between two ports.
    'part': [rect(2.5, 3.5, 15, 13, 2), path('M2.5 7.5H17.5'), rect(5.5, 10, 9, 4, 1)],
    'port': [rect(5.5, 3.5, 12, 13, 2), rect(3, 8, 5, 4, extra=FILL)],
    'connector': [rect(2.5, 7.5, 5, 5), rect(12.5, 7.5, 5, 5), path('M7.5 10H12.5')],
    # Elements without a tool, for the model tree. The model: a package with a triangle.
    'model': PACKAGE + [path('M10 8.5L13.5 14H6.5Z')],
    # Members: the compartment of a box they are shown in, filled.
    'attribute': [BOX, path('M2.5 7.5H17.5'), rect(2.5, 7.5, 15, 4.5, extra=FILL)],
    'operation': [BOX, path('M2.5 7.5H17.5M2.5 12H17.5'), band(12)],
    'enumerationLiteral': [BOX, path('M2.5 7.5H17.5'), rect(5, 9.5, 10, 2.5, 1, FILL),
                           path('M5.5 14H11.5')],
    'slot': [BOX, path('M6.5 7.5H13.5'), band(10.5)],
    'parameter': [path('M7 3.5C4.3 6.5 4.3 13.5 7 16.5M13 3.5C15.7 6.5 15.7 13.5 13 16.5'),
                  dot(10, 10, 2)],
    # A class with the dashed box of its template parameters on its corner.
    'templateParameter': [path('M10.5 5.5H2.5V16.5H14.5V8.5M2.5 9.5H10.5'),
                          dashed('M10.5 2.5H17.5V8.5H10.5Z')],
    'extensionPoint': [ellipse(10, 10, 7.5, 5.5), path('M2.5 10H17.5M6.5 12.75H13.5')],
    # Behaviors: a diagram frame with what they are made of.
    'interaction': FRAGMENT + [path('M5.5 12H13'), path('M12 9.5L15.5 12L12 14.5Z', FILL)],
    'activity': FRAGMENT + [dot(6, 12, 2), path('M8.5 12H15M12.5 9.5L15 12L12.5 14.5')],
    'stateMachine': FRAGMENT + [rect(6.5, 9.5, 8, 4.5, 2)],
    'interactionOperand': FRAGMENT + [dashed('M3 9.5H17'), band(11.5)],
    'region': [rect(2.5, 4.5, 15, 11, 3), path('M2.5 8.5H17.5'), dashed('M10 9V15')],
    # A structogram: an instruction above an alternative with its two branches.
    'structogram': [rect(2.5, 2.5, 15, 15, 1), path('M2.5 6.5H17.5M2.5 11.5H17.5'),
                    path('M2.5 6.5L10 11.5L17.5 6.5M10 11.5V17.5')],
    # Structogram elements, drawn as Structorizer draws them
    'nsdInstruction': [rect(2.5, 5.5, 15, 9), path('M5.5 10H14.5')],
    'nsdCall': [rect(2.5, 5.5, 15, 9), path('M5 5.5V14.5M15 5.5V14.5')],
    'nsdJump': [path('M5.5 5.5H17.5V14.5H5.5L2.5 10Z'), path('M8.5 10H14.5')],
    'nsdAlternative': NSD_ALTERNATIVE,
    'nsdCase': [rect(2.5, 3.5, 15, 13), path('M2.5 3.5L13.5 9.5L17.5 3.5M2.5 9.5H17.5'),
                path('M6.5 9.5V16.5M10 9.5V16.5M13.5 9.5V16.5')],
    'nsdFor': NSD_LOOP + [path('M9.5 5.5H14.5')],
    'nsdWhile': NSD_LOOP,
    'nsdRepeat': [path('M2.5 3.5H6.5V12.5H17.5V16.5H2.5Z'), rect(6.5, 3.5, 11, 9)],
    'nsdForever': [path('M2.5 3.5H17.5V7H6.5V13H17.5V16.5H2.5Z'), rect(6.5, 7, 11, 6)],
    'nsdTry': [rect(2.5, 3.5, 15, 13), rect(5.5, 6.5, 12, 4), path('M2.5 13.5H17.5')],
    'nsdParallel': [rect(2.5, 3.5, 15, 13), path('M2.5 6.5H17.5M2.5 13.5H17.5'),
                    path('M7.5 6.5V13.5M12.5 6.5V13.5')],
    'nsdColor': [rect(2.5, 4.5, 15, 11), rect(5.5, 7.5, 9, 5, extra=FILL)],
    'nsdDisabled': [rect(2.5, 4.5, 15, 11), path('M2.5 10.5L8.5 4.5M5.5 15.5L16.5 4.5'),
                    path('M11.5 15.5L17.5 9.5')],
    'nsdAddBranch': [rect(2.5, 3.5, 10, 13), path('M7.5 3.5V16.5'),
                     path('M15.5 7.5V13.5M12.5 10.5H18.5')],
    'nsdRemoveBranch': [rect(2.5, 3.5, 10, 13), path('M7.5 3.5V16.5'), path('M13.5 10.5H18.5')],
    # Edges
    'association': [path('M2.5 9.5H17.5')],
    'navigableAssociation': NAVIGABLE,
    'aggregation': [path('M2.5 9.5H9.5'), path(DIAMOND)],
    'composition': [path('M2.5 9.5H9.5'), path(DIAMOND, FILL)],
    'associationClass': [rect(5.5, 2.5, 9, 7, 1), path('M5.5 5.5H14.5'), dashed('M10 10V15'),
                         path('M2.5 15.5H17.5')],
    'containment': [path('M2.5 9.5H11.5'), circle(14.5, 9.5, 3),
                    path('M14.5 7.5V11.5M12.5 9.5H16.5')],
    'link': [dot(3.5, 9.5, 1.5), path('M5 9.5H15'), dot(16.5, 9.5, 1.5)],
    'generalization': [path('M2.5 9.5H11.5'), path('M11.5 5.5L17.5 9.5L11.5 13.5Z')],
    'realization': [dashed('M11 9.5H2'), path('M11.5 5.5L17.5 9.5L11.5 13.5Z')],
    'dependency': [dashed('M17 9.5H2'), OPEN_HEAD],
    'requiredInterface': [path('M2.5 9.5H11.5'),
                          path('M15.5 5.5C13.29 5.5 11.5 7.29 11.5 9.5'
                               'C11.5 11.71 13.29 13.5 15.5 13.5')],
    'include': [circle(4.5, 9.5, 2), path('M12.5 9.5H7.5', dash(1.5, 1.5)),
                path('M10.5 7.5L12.5 9.5L10.5 11.5'), circle(15.5, 9.5, 2)],
    'extend': [circle(4.5, 9.5, 2), path('M7.5 9.5H12.5', dash(1.5, 1.5)),
               path('M9.5 7.5L7.5 9.5L9.5 11.5'), circle(15.5, 9.5, 2)],
    'noteAnchor': [path('M2.5 5.5H7.5L9.5 7.5V13.5H2.5Z'), dashed('M17.5 9.5H10')],
    'message': [path('M2.5 9.5H13'), MESSAGE_HEAD],
    'asyncMessage': NAVIGABLE,
    'reply': [dashed('M3 9.5H18'), path('M7.5 6L2.5 9.5L7.5 13')],
    'createMessage': [dashed('M11.5 9.5H2'), path('M8.5 7L12 9.5L8.5 12'),
                      rect(12.5, 6.5, 5, 6, 1)],
    'destroyMessage': [path('M2.5 9.5H9'), path('M8 7L11.5 9.5L8 12Z', FILL),
                       path('M13 7L18 12M13 12L18 7')],
    'foundMessage': [dot(4.5, 9.5, 2), path('M6.5 9.5H13'), MESSAGE_HEAD],
    'lostMessage': [path('M2.5 9.5H9.5'), path('M8.5 6.5L13 9.5L8.5 12.5Z', FILL),
                    dot(15.5, 9.5, 2)],
    # Sequence diagram switches: a numbered message, and messages numbered 1, 2 straight on.
    'messageNumbers': [path('M3 4.5L5 2.5V8.5'), dot(7.5, 8, 1), path('M10.5 5.5H17'),
                       path('M2.5 13.5H13'), path('M12.5 10.5L17.5 13.5L12.5 16.5Z', FILL)],
    'flatMessageNumbers': [path('M3 3.5L4.5 2V7'), path('M7.5 4.5H13'),
                           path('M12.5 2.5L16 4.5L12.5 6.5Z', FILL),
                           path('M2.5 11.5C2.5 10.3 3.4 9.5 4.5 9.5C5.6 9.5 6.5 10.3 6.5 11.3'
                                'C6.5 12.6 4.6 13.9 2.5 16.5H6.5'),
                           path('M9.5 14.5H15'), path('M14.5 12.5L18 14.5L14.5 16.5Z', FILL)],
    'flow': NAVIGABLE,
    'transition': NAVIGABLE,
    'freeLine': [path('M16 4L3.5 16.5', dash(2, 2)), path('M11.5 3.5H16.5V8.5')],
}

os.makedirs(OUT, exist_ok=True)
for name, parts in ICONS.items():
    body = '\n'.join(parts)
    with io.open(os.path.join(OUT, name + '.svg'), 'w', encoding='utf-8', newline='\n') as f:
        f.write(f'{HEAD}\n{body}\n</svg>\n')
print(len(ICONS), 'icons')
