// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface ERDiagramEditor extends com.change_vision.jude.api.inf.editor.BasicDiagramEditor {
    public abstract com.change_vision.jude.api.inf.model.IERDiagram createERDiagram(com.change_vision.jude.api.inf.model.IERPackage p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.ILinkPresentation createLinkPresentation(com.change_vision.jude.api.inf.model.IElement p0, com.change_vision.jude.api.inf.presentation.INodePresentation p1, com.change_vision.jude.api.inf.presentation.INodePresentation p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createNodePresentation(com.change_vision.jude.api.inf.model.IElement p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createSubtypeRelationshipGroup(com.change_vision.jude.api.inf.presentation.ILinkPresentation[] p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setDiagram(com.change_vision.jude.api.inf.model.IDiagram p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
