// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface BasicDiagramEditor extends com.change_vision.jude.api.inf.editor.DiagramEditor {
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createNote(java.lang.String p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.ILinkPresentation createNoteAnchor(com.change_vision.jude.api.inf.presentation.INodePresentation p0, com.change_vision.jude.api.inf.presentation.IPresentation p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createText(java.lang.String p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createRect(java.awt.geom.Point2D p0, double p1, double p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
