// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface CompositeStructureModelEditor {
    public abstract com.change_vision.jude.api.inf.model.IRealization createRealization(com.change_vision.jude.api.inf.model.IPort p0, com.change_vision.jude.api.inf.model.IClass p1, java.lang.String p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.IUsage createUsage(com.change_vision.jude.api.inf.model.IPort p0, com.change_vision.jude.api.inf.model.IClass p1, java.lang.String p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
