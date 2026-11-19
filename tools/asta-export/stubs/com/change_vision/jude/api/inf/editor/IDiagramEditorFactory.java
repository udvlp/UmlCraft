// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface IDiagramEditorFactory {
    public abstract com.change_vision.jude.api.inf.editor.ClassDiagramEditor getClassDiagramEditor() throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
    public abstract com.change_vision.jude.api.inf.editor.CompositeStructureDiagramEditor getCompositeStructureDiagramEditor() throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
    public abstract com.change_vision.jude.api.inf.editor.UseCaseDiagramEditor getUseCaseDiagramEditor() throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
    public abstract com.change_vision.jude.api.inf.editor.StateMachineDiagramEditor getStateMachineDiagramEditor() throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
    public abstract com.change_vision.jude.api.inf.editor.SequenceDiagramEditor getSequenceDiagramEditor() throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
    public abstract com.change_vision.jude.api.inf.editor.ERDiagramEditor getERDiagramEditor() throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
    public abstract com.change_vision.jude.api.inf.editor.ActivityDiagramEditor getActivityDiagramEditor() throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
}
