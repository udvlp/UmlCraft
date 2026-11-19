// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface IModelEditorFactory {
    public abstract com.change_vision.jude.api.inf.editor.BasicModelEditor getBasicModelEditor() throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.editor.ERModelEditor getERModelEditor() throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.editor.UseCaseModelEditor getUseCaseModelEditor() throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.editor.CompositeStructureModelEditor getCompositeStructureModelEditor() throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
