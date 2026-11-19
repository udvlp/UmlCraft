// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.project;

public interface ProjectAccessor {
    public abstract com.change_vision.jude.api.inf.model.IModel getProject() throws com.change_vision.jude.api.inf.exception.ProjectNotFoundException;
    public abstract void close() throws java.lang.RuntimeException;
    public abstract void create(java.lang.String p0) throws java.io.IOException;
    public abstract void save() throws com.change_vision.jude.api.inf.exception.LicenseNotFoundException, com.change_vision.jude.api.inf.exception.ProjectNotFoundException, java.io.IOException, com.change_vision.jude.api.inf.exception.ProjectLockedException;
    public abstract com.change_vision.jude.api.inf.editor.IModelEditorFactory getModelEditorFactory();
    public abstract com.change_vision.jude.api.inf.editor.IDiagramEditorFactory getDiagramEditorFactory();
}
