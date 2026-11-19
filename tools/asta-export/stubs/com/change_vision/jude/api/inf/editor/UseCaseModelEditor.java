// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public interface UseCaseModelEditor {
    public abstract com.change_vision.jude.api.inf.model.IClass createActor(com.change_vision.jude.api.inf.model.IPackage p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.IUseCase createUseCase(com.change_vision.jude.api.inf.model.IPackage p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.IExtentionPoint createExtensionPoint(com.change_vision.jude.api.inf.model.IUseCase p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.IExtend createExtend(com.change_vision.jude.api.inf.model.IUseCase p0, com.change_vision.jude.api.inf.model.IUseCase p1, java.lang.String p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.IInclude createInclude(com.change_vision.jude.api.inf.model.IUseCase p0, com.change_vision.jude.api.inf.model.IUseCase p1, java.lang.String p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
