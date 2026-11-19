// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface DiagramEditor {
    public abstract com.change_vision.jude.api.inf.model.IDiagram getDiagram();
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createText(java.lang.String p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.ILinkPresentation createLine(java.awt.geom.Point2D p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
