# Premiers pas avec UmlCraft

[English](GETTING-STARTED.md) · [Deutsch](GETTING-STARTED.de.md)

Ce guide est pour vous si vous n'avez jamais utilisé UmlCraft, ni un outil UML de ce genre. Il
prend environ un quart d'heure. Tout ce que sait faire UmlCraft est décrit dans le
[README](README.md) (en anglais).

## L'idée à comprendre en premier

UmlCraft ressemble à un logiciel de dessin, mais n'en est pas un. Un logiciel de dessin retient
des formes : un rectangle avec un peu de texte dedans, une ligne entre deux rectangles. UmlCraft
retient un **modèle** : les classes, acteurs, états et autres éléments de ce que vous décrivez,
et les relations entre eux. Un **diagramme** est une vue sur une partie de ce modèle.

Pensez aux personnes d'un album photo. Chaque personne existe une seule fois, chaque photo en
montre quelques-unes. La même personne peut figurer sur beaucoup de photos. La retirer d'une
photo ne la fait pas disparaître des autres, et encore moins du monde.

Dans UmlCraft :

- Une classe `Client` existe **une seule fois** dans le modèle, quel que soit le nombre de
  diagrammes qui la montrent.
- Chaque diagramme qui montre `Client` montre la **même** classe. Renommez-la ou donnez-lui un
  attribut sur un diagramme, et tous les autres diagrammes affichent la modification.
- Une ligne entre deux classes est une **relation** du modèle, pas un simple trait. Elle
  appartient aux deux classes, pas au dessin.
- L'**arbre du modèle**, à gauche, liste tout ce que contient le modèle, y compris ce qu'aucun
  diagramme ne montre. C'est le vrai contenu de votre projet. Les diagrammes en sont des vues.

La question « Qu'y a-t-il sur mon diagramme ? » a donc deux volets : ce qui existe dans le
modèle, et ce que ce diagramme en montre, où et comment.

| Ce que vous voulez | Dans un logiciel de dessin | Dans UmlCraft |
| --- | --- | --- |
| Montrer une classe sur un deuxième diagramme | Copier et coller le rectangle | Faire glisser la classe de l'arbre du modèle sur le diagramme, ou la copier et la coller avec Ctrl+Maj+V |
| Renommer une classe partout | Modifier chaque rectangle | La renommer une fois |
| Enlever un rectangle d'un diagramme | Le supprimer | **Retirer du diagramme** (Ctrl+Suppr) : la classe reste dans le modèle et sur les autres diagrammes |
| Se débarrasser complètement d'une classe | La supprimer sur chaque page | **Supprimer du modèle** (Suppr) : elle disparaît de tous les diagrammes, avec ses lignes |
| Créer une deuxième classe, indépendante | Copier et coller | Copier et coller (Ctrl+C, Ctrl+V) : la copie est un nouvel élément |

**Supprimer** et **Retirer du diagramme** sont les deux commandes à retenir. La touche Suppr
supprime du modèle. Si vous vouliez seulement mettre de l'ordre dans un diagramme, appuyez
sur Ctrl+Z et utilisez plutôt Ctrl+Suppr. Toute modification peut être annulée.

## La fenêtre

- **Barre de titre**, tout en haut : Enregistrer, Annuler, Rétablir et le nom du projet, et à
  droite À propos d'UmlCraft et les paramètres (la roue dentée).
- **Ruban**, juste en dessous, avec ses onglets en haut. **Fichier** ouvre une page par-dessus
  la fenêtre pour créer, ouvrir, enregistrer, importer et exporter. Retour ou Échap ramène au
  diagramme. **Édition** réunit le presse-papiers, la suppression, l'alignement, le style des
  lignes, la disposition et Nouveau diagramme, **Affichage** le zoom, le thème clair ou sombre
  et la langue. Tant qu'un diagramme est ouvert, un dernier onglet, nommé d'après son type,
  contient ses éléments et ses relations.
- **Modèle**, le panneau de gauche : l'arbre du modèle. Un clic sur un diagramme l'ouvre, un
  clic sur un élément le sélectionne.
- **Zone de dessin**, au milieu : le diagramme ouvert.
- **Propriétés**, le panneau de droite : tout ce qui concerne la sélection, comme son nom, ses
  attributs ou la couleur de son rectangle.

La zone de dessin se trouve entre les deux panneaux. Faites glisser le bord intérieur d'un
panneau pour l'élargir ou le rétrécir.

Se déplacer dans la zone de dessin : la molette de la souris fait défiler, Ctrl+molette (ou deux
doigts sur un pavé tactile) zoome, et le bouton du milieu ou le bouton droit maintenu enfoncé
déplace la vue. La zone de zoom, en bas, a un bouton qui fait tenir tout le diagramme dans la
fenêtre (Ctrl+Maj+0).

## Un coup d'œil au projet d'exemple

**Fichier › Nouveau › Projet d'exemple** ouvre un projet terminé, une location de vélos appelée
Velo, avec un diagramme de chaque type. Il s'ouvre sur le diagramme de classes *Domain model*.
Parcourez les diagrammes dans l'arbre du modèle. Puis mettez en pratique l'idée ci-dessus :

1. Cliquez sur la classe `Station` dans *Domain model*. Le panneau des propriétés l'affiche.
2. Ouvrez le diagramme *Saturday, 9:12* dans le paquetage *rental*. Il montre des objets, parmi
   lesquels `central : Station` : un objet de cette classe précisément, avec un emplacement pour
   chacun de ses attributs.
3. Revenez à *Domain model* et renommez `Station` en `Hub` (double-cliquez sur son nom). Sur
   *Saturday, 9:12*, l'objet affiche maintenant `central : Hub`. Donnez un autre attribut à la
   classe, et l'objet reçoit un emplacement pour cet attribut.
4. Annulez avec Ctrl+Z.

## Votre premier diagramme

1. **Fichier › Nouveau › Projet vide** (Ctrl+N). Un nouveau projet est vide, sans diagramme.
2. **Édition › Nouveau diagramme › Diagramme de classes**. Le ruban a maintenant un onglet
   *Diagramme de classes* avec ses outils, et l'arbre du modèle montre le nouveau diagramme.
3. Choisissez **Classe** dans le ruban et cliquez dans le diagramme. Le nom de la nouvelle
   classe est aussitôt prêt à la saisie : tapez `Client` et appuyez sur Entrée.
4. Ajoutez de la même façon une deuxième classe `Commande`.
5. **Attributs et opérations** : double-cliquez sous les membres d'une classe, ou faites un clic
   droit dessus et choisissez *Ajouter un attribut* ou *Ajouter une opération*. Écrivez-les
   comme on les écrit en UML : `- nom : String` ou `+ total() : int`. Entrée ouvre la ligne suivante,
   Échap arrête la saisie.
6. **Une relation** : survolez `Client`. Une poignée en forme de flèche apparaît à côté.
   Faites-la glisser sur `Commande` et choisissez *Association* dans le menu. Vous pouvez aussi
   choisir un outil de relation dans le ruban, comme *Association*, et faire glisser d'une
   classe à l'autre. Double-cliquez près d'une extrémité de la ligne pour lui donner un nom de
   rôle ou une multiplicité comme `0..*`.
7. **Enregistrez** avec Ctrl+S. Les projets UmlCraft sont des fichiers `.ucraft`.

Regardez maintenant l'arbre du modèle : `Client`, `Commande`, leurs attributs et l'association
y figurent tous, à côté du diagramme.

## La même classe sur un deuxième diagramme

1. Encore une fois **Édition › Nouveau diagramme › Diagramme de classes**.
2. Faites glisser `Client` de l'arbre du modèle sur le nouveau diagramme, puis `Commande`.
   L'association suit d'elle-même, car elle existe entre les deux classes dans le modèle.
3. Renommez `Commande` en `Achat` sur ce diagramme (double-cliquez sur son nom), puis ouvrez le
   premier diagramme : il affiche lui aussi `Achat`.
4. Sélectionnez `Client` et appuyez sur **Ctrl+Suppr** (Retirer du diagramme). La classe a
   disparu de ce diagramme, mais elle est toujours dans l'arbre du modèle et sur le premier
   diagramme.
5. Sélectionnez `Achat` et appuyez sur **Suppr**. La classe a disparu des deux diagrammes et de
   l'arbre du modèle, avec l'association. Appuyez sur Ctrl+Z pour la faire revenir.

Il en va de même pour tous les types de diagrammes : cas d'utilisation et acteurs, états,
activités, composants, lignes de vie. Chaque type a son propre onglet dans le ruban, avec les
éléments et les relations qui ont un sens pour lui.

## Laisser UmlCraft s'occuper de la disposition

Vous n'avez pas à placer chaque rectangle ni à couder chaque ligne à la main.

- **Disposer automatiquement** (Ctrl+Maj+L) dispose tout le diagramme, ou seulement la
  sélection. Son menu propose « De haut en bas » ou « De gauche à droite ».
- **Tracer les lignes** (Ctrl+Maj+R) fait passer les lignes autour des rectangles, à angle
  droit.
- Les nouvelles lignes sont tracées pour vous et suivent leurs rectangles quand vous les
  déplacez, jusqu'à ce que vous en coudiez une à la main.

## Utiliser vos diagrammes hors d'UmlCraft

- **Fichier › Exporter › Diagramme en SVG** ou **Diagramme en PNG** enregistre le diagramme
  ouvert sous forme d'image.
- **Édition › Copier comme image** (Ctrl+Maj+C) copie le diagramme dans le presse-papiers :
  Word et PowerPoint le collent comme image vectorielle, les autres programmes comme image
  ordinaire.

## Les touches utiles

| Touche | Ce qu'elle fait |
| --- | --- |
| Ctrl+N, Ctrl+O, Ctrl+S | Nouveau projet, ouvrir, enregistrer |
| Ctrl+Z, Ctrl+Y | Annuler, rétablir |
| Suppr | Supprimer la sélection du modèle |
| Ctrl+Suppr | Retirer la sélection de ce diagramme seulement |
| Ctrl+C, Ctrl+X, Ctrl+V | Copier, couper, coller comme nouveaux éléments |
| Ctrl+Maj+V | Coller les mêmes éléments, par exemple sur un autre diagramme |
| Ctrl+Maj+L | Disposer automatiquement |
| Ctrl+Maj+R | Tracer les lignes |
| Ctrl+0, Ctrl+Maj+0 | Zoom à 100 %, faire tenir le diagramme dans la fenêtre |
| Ctrl+Maj+C | Copier le diagramme comme image |
| Ctrl+, | Paramètres |

Sur macOS, Cmd remplace Ctrl.

## Pour aller plus loin

- Les autres types de diagrammes : **Édition › Nouveau diagramme** les liste tous. Le projet
  d'exemple en contient un de chaque type.
- **Structogrammes** (diagrammes de Nassi-Shneiderman) :
  **Édition › Nouveau diagramme › Structogramme**.
- **Code source** : **Fichier › Importer › Code source** lit du code Java, C#, C++ ou Python
  et l'ajoute au modèle, à partir de fichiers isolés ou d'un dossier entier. Vous obtenez ainsi des
  classes dans l'arbre du modèle, mais pas encore de diagramme : faites-les glisser sur un
  diagramme.
- Le [README](README.md) (en anglais) décrit tout ce que sait faire UmlCraft.
