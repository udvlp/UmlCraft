// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface UseCaseDiagramEditor extends com.change_vision.jude.api.inf.editor.StructureDiagramEditor {
    public abstract com.change_vision.jude.api.inf.model.IUseCaseDiagram createUseCaseDiagram(com.change_vision.jude.api.inf.model.INamedElement p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
