// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface StructureDiagramEditor extends com.change_vision.jude.api.inf.editor.BasicDiagramEditor {
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createNodePresentation(com.change_vision.jude.api.inf.model.IElement p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.ILinkPresentation createLinkPresentation(com.change_vision.jude.api.inf.model.IElement p0, com.change_vision.jude.api.inf.presentation.INodePresentation p1, com.change_vision.jude.api.inf.presentation.INodePresentation p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
