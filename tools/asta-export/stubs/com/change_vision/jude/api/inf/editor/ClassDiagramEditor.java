// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface ClassDiagramEditor extends com.change_vision.jude.api.inf.editor.StructureDiagramEditor {
    public abstract com.change_vision.jude.api.inf.model.IClassDiagram createClassDiagram(com.change_vision.jude.api.inf.model.INamedElement p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.INodePresentation createInstanceSpecification(java.lang.String p0, java.awt.geom.Point2D p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.ILinkPresentation createInstanceSpecificationLink(com.change_vision.jude.api.inf.presentation.INodePresentation p0, com.change_vision.jude.api.inf.presentation.INodePresentation p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.presentation.IPresentation[] createAssociationClassPresentation(com.change_vision.jude.api.inf.model.IAssociationClass p0, com.change_vision.jude.api.inf.presentation.INodePresentation p1, com.change_vision.jude.api.inf.presentation.INodePresentation p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
