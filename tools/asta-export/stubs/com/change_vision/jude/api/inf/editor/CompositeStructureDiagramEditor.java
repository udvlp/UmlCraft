// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface CompositeStructureDiagramEditor extends com.change_vision.jude.api.inf.editor.StructureDiagramEditor {
    public abstract com.change_vision.jude.api.inf.model.ICompositeStructureDiagram createCompositeStructureDiagram(com.change_vision.jude.api.inf.model.INamedElement p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createStructuredClassPresentation(com.change_vision.jude.api.inf.model.IElement p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createStructuredClassPresentation(com.change_vision.jude.api.inf.model.IElement p0, com.change_vision.jude.api.inf.presentation.INodePresentation p1, java.awt.geom.Point2D p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createPortPresentation(com.change_vision.jude.api.inf.presentation.INodePresentation p0, com.change_vision.jude.api.inf.model.IPort p1, java.awt.geom.Point2D p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void showInterfacePresentations(com.change_vision.jude.api.inf.presentation.INodePresentation p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.ILinkPresentation createConnectorPresentation(com.change_vision.jude.api.inf.presentation.INodePresentation p0, com.change_vision.jude.api.inf.presentation.INodePresentation p1, java.lang.String p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.IDiagram getDiagram();
    public abstract void deletePresentation(com.change_vision.jude.api.inf.presentation.IPresentation p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
